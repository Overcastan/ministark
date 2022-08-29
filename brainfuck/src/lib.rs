//! Implementation inspired by https://github.com/Overv/bf

/// Opcodes determined by the lexer
#[derive(Debug, Clone)]
enum OpCode {
    IncrementPointer,
    DecrementPointer,
    Increment,
    Decrement,
    Write,
    Read,
    LoopBegin,
    LoopEnd,
}

#[derive(Debug, Clone)]
enum Instr {
    IncrementPointer,
    DecrementPointer,
    Increment,
    Decrement,
    Write,
    Read,
    Loop(Vec<Instr>),
}

/// Lexer turns the source code into a sequence of opcodes
fn lex(source: &str) -> Vec<OpCode> {
    let mut operations = Vec::new();

    for symbol in source.chars() {
        let op = match symbol {
            '>' => Some(OpCode::IncrementPointer),
            '<' => Some(OpCode::DecrementPointer),
            '+' => Some(OpCode::Increment),
            '-' => Some(OpCode::Decrement),
            '.' => Some(OpCode::Write),
            ',' => Some(OpCode::Read),
            '[' => Some(OpCode::LoopBegin),
            ']' => Some(OpCode::LoopEnd),
            _ => None,
        };

        // Non-opcode characters are comments
        match op {
            Some(op) => operations.push(op),
            None => (),
        }
    }

    operations
}

fn parse(opcodes: &[OpCode]) -> Vec<Instr> {
    let mut program = Vec::new();
    let mut loop_stack = 0;
    let mut loop_start = 0;

    for (i, op) in opcodes.iter().enumerate() {
        if loop_stack == 0 {
            let instr = match op {
                OpCode::IncrementPointer => Some(Instr::IncrementPointer),
                OpCode::DecrementPointer => Some(Instr::DecrementPointer),
                OpCode::Increment => Some(Instr::Increment),
                OpCode::Decrement => Some(Instr::Decrement),
                OpCode::Write => Some(Instr::Write),
                OpCode::Read => Some(Instr::Read),
                OpCode::LoopBegin => {
                    loop_start = i;
                    loop_stack += 1;
                    None
                }
                OpCode::LoopEnd => panic!("loop ending at #{} has no beginning", i),
            };

            match instr {
                Some(instr) => program.push(instr),
                None => (),
            }
        } else {
            match op {
                OpCode::LoopBegin => {
                    loop_stack += 1;
                }
                OpCode::LoopEnd => {
                    loop_stack -= 1;
                    if loop_stack == 0 {
                        program.push(Instr::Loop(parse(&opcodes[loop_start + 1..i])));
                    }
                }
                _ => (),
            }
        }
    }

    if loop_stack != 0 {
        panic!(
            "loop that starts at #{} has no matching ending!",
            loop_start
        );
    }

    program
}

/// Executes a program that was previously parsed
fn run(
    instrs: &[Instr],
    input: &mut impl std::io::Read,
    output: &mut impl std::io::Write,
    tape: &mut [u8],
    data_pointer: &mut usize,
) {
    for instr in instrs {
        match instr {
            Instr::IncrementPointer => *data_pointer += 1,
            Instr::DecrementPointer => *data_pointer -= 1,
            Instr::Increment => tape[*data_pointer] += 1,
            Instr::Decrement => tape[*data_pointer] -= 1,
            Instr::Write => output
                .write_all(&tape[*data_pointer..*data_pointer + 1])
                .expect("failed to write output"),
            Instr::Read => {
                let mut x = [0u8; 1];
                input.read_exact(&mut x).expect("failed to read input");
                tape[*data_pointer] += x[0];
            }
            Instr::Loop(nested_instrs) => {
                while tape[*data_pointer] != 0 {
                    run(nested_instrs, input, output, tape, data_pointer);
                }
            }
        }
    }
}

pub fn execute(source: &str, input: &mut impl std::io::Read, output: &mut impl std::io::Write) {
    let opcodes = lex(source);
    let program = parse(&opcodes);
    let mut tape: Vec<u8> = vec![0; 1024];
    let mut data_pointer = 512;
    run(&program, input, output, &mut tape, &mut data_pointer);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_world() {
        let source = "
            +++++ +++++             initialize counter (cell #0) to 10
            [                       use loop to set 70/100/30/10
                > +++++ ++              add  7 to cell #1
                > +++++ +++++           add 10 to cell #2
                > +++                   add  3 to cell #3
                > +                     add  1 to cell #4
            <<<< -                  decrement counter (cell #0)
            ]
            > ++ .                  print 'H'
            > + .                   print 'e'
            +++++ ++ .              print 'l'
            .                       print 'l'
            +++ .                   print 'o'
            > ++ .                  print ' '
            << +++++ +++++ +++++ .  print 'W'
            > .                     print 'o'
            +++ .                   print 'r'
            ----- - .               print 'l'
            ----- --- .             print 'd'
            > + .                   print '!'
            > .                     print '\n'
        ";
        let mut output = Vec::new();

        execute(source, &mut std::io::empty(), &mut output);

        assert_eq!(output, "Hello World!\n".as_bytes());
        // print()
    }
}