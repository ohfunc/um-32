use std::collections::HashMap;
use std::env;
use std::io::{self, ErrorKind, Read};

static OPCODES: [&str; 14] = [
    "cmove", "aidx", "amend", "add", "mul", "div", "nand", "halt", "alloc", "aban", "out", "in",
    "load", "orth",
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
//         Figure 0. Platters

// Each bit may be the 0 bit or the 1 bit. Using the system of
// "unsigned 32-bit numbers" (see patent #4,294,967,295) the
// markings on these platters may also denote numbers.
type Platter = u32;
type Addr = u32;
type Register = u32;

struct UM32 {
    registers: [Register; 8],
    free: Vec<Addr>,
    heap: HashMap<Addr, Vec<Platter>>,
    finger: Addr,
    // Indices to the given registers for a given instruction.
    ra: usize,
    rb: usize,
    rc: usize,
    debug: bool,
}

impl UM32 {
    fn new(debug: bool) -> UM32 {
        // Eight distinct general-purpose registers, capable of holding one
        // platter each.
        // All registers shall be initialized with platters of value '0'.
        let registers: [u32; 8] = [0, 0, 0, 0, 0, 0, 0, 0];

        // A collection of arrays of platters, each referenced by a distinct
        // 32-bit identifier. One distinguished array is referenced by 0
        // and stores the "program." This array will be referred to as the
        // '0' array.
        let heap: HashMap<Addr, Vec<Platter>> = HashMap::new();

        UM32 {
            registers,
            heap: heap,
            free: Vec::new(),
            finger: 0,
            ra: 0,
            rb: 0,
            rc: 0,
            debug: debug,
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
        self.heap.insert(0, program);
    }

    fn run(&mut self) {
        if self.debug {
            println!(
                "{:>8}: {:<8} {:>8} {:<8} {:<8} {:<8}",
                "ip", "ins", "op", "ra", "rb", "rc"
            );
        }

        loop {
            let ip = self.finger;
            let ins = self.heap[&0][self.finger as usize];
            match self.spin(ip, ins) {
                Err(e) => {
                    eprintln!("Error: {}", e);
                    break;
                }
                _ => (),
            }

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

        if self.debug {
            println!(
                "{:08X}: {:08X} {:>8} {:08X} {:08X} {:08X}",
                ip,
                instruction,
                OPCODES[usize::try_from(opcode).unwrap()],
                self.registers[self.ra],
                self.registers[self.rb],
                self.registers[self.rc],
            );
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
            11 => return Err("unimplemented operation 'in'".to_string()),
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
        self.registers[self.ra] =
            self.heap.get(&self.registers[self.rb]).unwrap()[self.registers[self.rc] as usize];
    }

    // #2. Array Amendment.
    // The array identified by A is amended at the offset
    // in register B to store the value in register C.
    fn amend(&mut self) {
        self.heap
            .entry(self.registers[self.ra])
            .and_modify(|f| f[self.registers[self.rb] as usize] = self.registers[self.rc]);
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
                self.heap.insert(a, vec![0; s]);
                self.registers[self.rb] = a;
                return;
            }
            None => (),
        }

        // Allocate random addresses until one is empty.
        loop {
            let a = rand::random::<u32>();
            if a != 0 && !self.heap.contains_key(&a) {
                self.heap.insert(a, vec![0; s]);
                self.registers[self.rb] = a;
                break;
            }
        }
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
        print!("{}", char::from_u32(self.registers[self.rc]).unwrap());
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

        let v = self.heap.get(&self.registers[self.rb]).cloned().unwrap();
        self.heap.insert(0, v);
    }

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
    //
    //     Figure 3. Special Operators
    //
    // #13. Orthography.
    //
    // The value indicated is loaded into the register A
    // forthwith.
    fn ortho(&mut self, instruction: Platter) {
        self.ra = (instruction >> 25 & 0b111u32) as usize;
        let value = instruction & 0b1111111111111111111111111u32;
        self.registers[self.ra] = value;
    }
}

fn main() {
    let debug = env::var("DEBUG").is_ok();
    let mut um = UM32::new(debug);
    um.parse(io::stdin());
    um.run();
}
