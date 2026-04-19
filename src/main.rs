use std::fs::File;
use std::io::{self, BufWriter, ErrorKind, Read, Write};
use std::{env};

const NEWLINE: u8 = b'\n';
const EOI: u32 = 0xFFFFFFFF;

const OPCODES: [&str; 14] = [
    "CMOVE", "AIDX", "AMEND", "ADD", "MUL", "DIV", "NAND", "HALT", "ALLOC", "ABAN", "OUT", "IN",
    "LOAD", "ORTH",
];

// An infinite supply of sandstone platters, with room on each
// for thirty-two small marks, which we call "bits."
//
//                     least meaningful bit
//                                         |
//                                         v
//         .--------------------------------.
//         |VUTSRQPONMLKJIHGFEDCBA9876543210|
//         `--------------------------------'
//         ^
//         |
//         most meaningful bit
//
// Each bit may be the 0 bit or the 1 bit. Using the system of
// "unsigned 32-bit numbers" (see patent #4,294,967,295) the
// markings on these platters may also denote numbers.
type Platter = u32;
type Addr = u32;
type Register = u32;

struct UM32 {
    // Eight distinct general-purpose registers, capable of holding one
    // platter each.
    // All registers shall be initialized with platters of value '0'.
    registers: [Register; 8],
    free: Vec<Addr>,
    // A collection of arrays of platters, each referenced by a distinct
    // 32-bit identifier. One distinguished array is referenced by 0
    // and stores the "program." This array will be referred to as the
    // '0' array.
    heap: Vec<Option<Vec<Platter>>>,
    finger: Addr,
    // Indices to the given registers for the current instruction.
    ra: usize,
    rb: usize,
    rc: usize,
    decomp: bool,
    ibuf: String,
    decomp_file: Option<BufWriter<File>>,
}

impl UM32 {
    fn new(debug: bool, debug_file: Option<BufWriter<File>>) -> UM32 {
        UM32 {
            registers: [0; 8],
            heap: vec![None],
            free: Vec::new(),
            finger: 0,
            ra: 0,
            rb: 0,
            rc: 0,
            decomp: debug,
            ibuf: String::new(),
            decomp_file: debug_file,
        }
    }

    fn parse<R: Read>(&mut self, mut reader: R) {
        // The machine shall be initialized with a '0' array whose contents
        // shall be read from a "program" scroll.  The execution finger shall
        // point to the first platter of the '0' array, which has offset zero.
        let mut program: Vec<Platter> = Vec::new();
        let mut buf = [0; 4];
        loop {
            match reader.read_exact(&mut buf) {
                Ok(_) => program.push(u32::from_be_bytes(buf)),
                Err(e) => match e.kind() {
                    ErrorKind::UnexpectedEof => break,
                    _ => eprintln!("Error: {}", e),
                },
            }
        }
        self.heap[0] = Some(program);
    }

    fn run(&mut self) -> Result<(), String> {
        loop {
            let program = self.heap[0].as_mut().unwrap();
            if self.finger as usize >= program.len() {
                return Err("end of program reached before HALT instruction".to_string());
            }

            let ip = self.finger;
            let ins = program[self.finger as usize];
            self.spin(ip, ins)?;

            // Determine whether we've just performed an ortho operation.
            // If so, don't advance the finger.
            if ip != self.finger {
                continue;
            }
            self.finger += 1;
        }
    }

    // Registers A, B, and C.
    //                         A     C
    //                         |     |
    //                         vvv   vvv
    // .--------------------------------.
    // |VUTSRQPONMLKJIHGFEDCBA9876543210|
    // `--------------------------------'
    //  ^^^^                      ^^^
    //  |                         |
    //  operator number           B
    fn read_registers(&mut self, platter: Platter) {
        self.ra = (platter >> 6 & 0b111u32) as usize;
        self.rb = (platter >> 3 & 0b111u32) as usize;
        self.rc = (platter & 0b111u32) as usize;
    }

    fn spin(&mut self, ip: Addr, instruction: Platter) -> Result<(), String> {
        let opcode = instruction >> 28;
        self.read_registers(instruction);

        if self.decomp {
            match OPCODES.get(usize::try_from(opcode).unwrap()) {
                Some(op) => write!(
                    self.decomp_file.as_mut().unwrap(),
                    "0x{:08X}: {:<05} {} {} {}\n",
                    ip,
                    op,
                    self.ra,
                    self.rb,
                    self.rc,
                )
                .unwrap(),
                None => write!(
                    self.decomp_file.as_mut().unwrap(),
                    "0x{:08X}: 0x{:08X}\n",
                    ip,
                    instruction
                ).unwrap(),
            };
            return Ok(());
        }

        match opcode {
            0 => self.cmove(),
            1 => self.aidx(),
            2 => self.amend(),
            3 => self.add(),
            4 => self.mul(),
            5 => self.div(),
            6 => self.nand(),
            7 => std::process::exit(0), // halt instruction
            8 => self.alloc(),
            9 => self.aban(),
            10 => self.out(),
            11 => self.input(),
            12 => self.load(),
            13 => self.ortho(instruction),
            _ => {
                return Err(format!(
                    "unknown opcode {} at ip {:X} on platter {:X}",
                    opcode, ip, instruction
                ));
            }
        }

        Ok(())
    }

    // Operator #0. Conditional Move.
    // The register A receives the value in register B,
    // unless the register C contains 0.
    fn cmove(&mut self) {
        if self.registers[self.rc] != 0 {
            self.registers[self.ra] = self.registers[self.rb]
        }
    }

    // #1. Array Index.
    // The register A receives the value stored at offset
    // in register C in the array identified by B.
    fn aidx(&mut self) {
        self.registers[self.ra] = self.heap[self.registers[self.rb] as usize]
            .as_mut()
            .unwrap()[self.registers[self.rc] as usize];
    }

    // #2. Array Amendment.
    // The array identified by A is amended at the offset
    // in register B to store the value in register C.
    fn amend(&mut self) {
        self.heap[self.registers[self.ra] as usize]
            .as_mut()
            .unwrap()[self.registers[self.rb] as usize] = self.registers[self.rc];
    }

    // #3. Addition.
    // The register A receives the value in register B plus
    // the value in register C, modulo 2^32.
    fn add(&mut self) {
        self.registers[self.ra] = self.registers[self.rb]
            .overflowing_add(self.registers[self.rc])
            .0;
    }

    // #4. Multiplication.
    // The register A receives the value in register B times
    // the value in register C, modulo 2^32.
    fn mul(&mut self) {
        self.registers[self.ra] = self.registers[self.rb].wrapping_mul(self.registers[self.rc]);
    }

    // #5. Division.
    // The register A receives the value in register B
    // divided by the value in register C, if any, where
    // each quantity is treated as an unsigned 32 bit number.
    fn div(&mut self) {
        if self.registers[self.rc] != 0 {
            self.registers[self.ra] = self.registers[self.rb] / self.registers[self.rc];
        }
    }

    // #6. Not-And.
    // Each bit in the register A receives the 1 bit if
    // either register B or register C has a 0 bit in that
    // position.  Otherwise the bit in register A receives
    // the 0 bit.
    fn nand(&mut self) {
        self.registers[self.ra] = !(self.registers[self.rb] & self.registers[self.rc]);
    }

    // #8. Allocation.
    // A new array is created with a capacity of platters
    // commensurate to the value in the register C. This
    // new array is initialized entirely with platters
    // holding the value 0. A bit pattern not consisting of
    // exclusively the 0 bit, and that identifies no other
    // active allocated array, is placed in the B register.
    fn alloc(&mut self) {
        let s = self.registers[self.rc] as usize;

        // Fast path: allocate from our list of free "pages".
        match self.free.pop() {
            Some(a) => {
                self.heap[a as usize] = Some(vec![0; s]);
                self.registers[self.rb] = a;
                return;
            }
            None => (),
        }

        self.heap.push(Some(vec![0; s]));
        self.registers[self.rb] = (self.heap.len() - 1) as u32;
    }

    // #9. Abandonment.
    // The array identified by the register C is abandoned.
    // Future allocations may then reuse that identifier.
    fn aban(&mut self) {
        self.free.push(self.registers[self.rc]);
    }

    // #10. Output.
    // The value in the register C is displayed on the console
    // immediately. Only values between and including 0 and 255
    // are allowed.
    fn out(&self) {
        io::stdout()
            .write(&[self.registers[self.rc] as u8])
            .unwrap();
    }

    // #11. Input.
    // The universal machine waits for input on the console.
    // When input arrives, the register C is loaded with the
    // input, which must be between and including 0 and 255.
    // If the end of input has been signaled, then the
    // register C is endowed with a uniform value pattern
    // where every place is pregnant with the 1 bit.
    fn input(&mut self) {
        // We create an input buffer so we don't have to deal with terminal raw mode, etc.
        if self.ibuf.is_empty() {
            io::stdin().read_line(&mut self.ibuf).unwrap();
        }

        let c = self.ibuf.remove(0);
        if c as u8 == NEWLINE {
            self.registers[self.rc] = EOI;
            return;
        }

        self.registers[self.rc] = c as u32;
    }

    // #12. Load Program.
    //
    // The array identified by the B register is duplicated
    // and the duplicate shall replace the '0' array,
    // regardless of size. The execution finger is placed
    // to indicate the platter of this array that is
    // described by the offset given in C, where the value
    // 0 denotes the first platter, 1 the second, et
    // cetera.
    //
    // The '0' array shall be the most sublime choice for
    // loading, and shall be handled with the utmost
    // velocity.
    fn load(&mut self) {
        self.finger = self.registers[self.rc];
        // Don't bother duplicating if we're just performing a jump.
        if self.registers[self.rb] == 0 {
            return;
        }

        let v = self.heap[self.registers[self.rb] as usize].clone().unwrap();
        self.heap[0] = Some(v);
    }

    // #13. Orthography.
    //
    // The value indicated is loaded into the register A
    // forthwith.
    //
    // One special operator does not describe registers in the same way.
    // Instead the three bits immediately less significant than the four
    // instruction indicator bits describe a single register A. The
    // remainder twenty five bits indicate a value, which is loaded
    // forthwith into the register A.
    //
    //          A
    //          |
    //          vvv
    //     .--------------------------------.
    //     |VUTSRQPONMLKJIHGFEDCBA9876543210|
    //     `--------------------------------'
    //      ^^^^   ^^^^^^^^^^^^^^^^^^^^^^^^^
    //      |      |
    //      |      value
    //      |
    //      operator number
    fn ortho(&mut self, instruction: Platter) {
        self.ra = (instruction >> 25 & 0b111u32) as usize;
        let value = instruction & 0b1111111111111111111111111u32;
        self.registers[self.ra] = value;
    }
}

fn main() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        return Err("Provide a UM-32 program as an argument.".to_string());
    }

    let program = File::open(args[1].as_str()).unwrap();
    let decomp = env::var("DECOMP").is_ok();

    let mut decomp_file: Option<BufWriter<File>> = None;
    if decomp {
        decomp_file = Some(BufWriter::new(
            File::create(env::var("DECOMP_FILE").unwrap()).unwrap(),
        ));
    }

    let mut um = UM32::new(decomp, decomp_file);
    um.parse(program);
    um.run()
}
