#![feature(int_log)]

use ark_ff_optimized::fp64::Fp;
use brainfuck::stark::compile;
use brainfuck::stark::SimulationMatrices;
use legacy_algebra::number_theory_transform::number_theory_transform;
use legacy_algebra::Felt;
use legacy_algebra::StarkFelt;
use num_traits::One;
use num_traits::Zero;
use stark::protocol::StandardProofStream;
use stark::BrainFuckStark;
use stark::Config;
use std::fs;
use std::mem::size_of;

const FIB_TO_100_SOURCE: &str = "
+++++++++++
>+>>>>++++++++++++++++++++++++++++++++++++++++++++
>++++++++++++++++++++++++++++++++<<<<<<[>[>>>>>>+>
+<<<<<<<-]>>>>>>>[<<<<<<<+>>>>>>>-]<[>++++++++++[-
<-[>>+>+<<<-]>>>[<<<+>>>-]+<[>[-]<[-]]>[<<[>>>+<<<
-]>>[-]]<<]>>>[>>+>+<<<-]>>>[<<<+>>>-]+<[>[-]<[-]]
>[<<+>>[-]]<<<<<<<]>>>>>[+++++++++++++++++++++++++
+++++++++++++++++++++++.[-]]++++++++++<[->-<]>++++
++++++++++++++++++++++++++++++++++++++++++++.[-]<<
<<<<<<<<<<[>>>+>+<<<<-]>>>>[<<<<+>>>>-]<-[>>.>.<<<
[-]]<<[>>+>+<<<-]>>>[<<<+>>>-]<<[<+>-]>[<+>-]<<<-]
";

const HELLO_WORLD_SOURCE: &str = "
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

const TINY: &str = "
+++++++++.
";

// pub type Fq = ark_ff::fields::Fp64<ark_ff::fields::MontBackend<FqConfig, 6>>;
// pub const FQ_ONE: Fq = ark_ff::MontFp!("1");
// pub const FQ_ZERO: Fq = ark_ff::MontFp!("0");

struct StarkConfig;
impl Config for StarkConfig {
    type Fp = Fp;
    type Fx = Fp;

    const EXPANSION_FACTOR: usize = 4;
    const SECURITY_LEVEL: usize = 128;
    const NUM_RANDOMIZERS: usize = 0;
}

#[test]
fn hello_world() {
    let program = compile(HELLO_WORLD_SOURCE); //TINY); //HELLO_WORLD_SOURCE);
    let mut output = Vec::new();
    let SimulationMatrices {
        processor: processor_matrix,
        instruction: instruction_matrix,
        input: input_matrix,
        output: output_matrix,
        memory: memory_matrix,
    } = brainfuck::stark::simulate::<Fp>(&program, &mut std::io::empty(), &mut output);

    let running_time = processor_matrix.len();
    println!("Running time: {running_time}");
    // let memory_length = memory_matrix.len();

    let mut proof_stream = StandardProofStream::<Fp>::new();
    let mut bfs = BrainFuckStark::new(StarkConfig);
    let res = bfs.prove(
        processor_matrix,
        memory_matrix,
        instruction_matrix,
        input_matrix,
        output_matrix,
        &mut proof_stream,
    );

    println!(
        "Output: {}",
        String::from_utf8(output).unwrap(),
        // output.iter().map(|v| v as char).collect::<Vec<char>>()
    );
    println!("Size: {}", res.len());
    fs::write("./proof.json", &res).unwrap();
}

#[test]
fn verify() {
    let proof = fs::read("./proof.json").unwrap();
    let mut proof_stream = StandardProofStream::<Fp>::new();
    let mut bfs = BrainFuckStark::new(StarkConfig);
    // let indices_seed = 5; // proof_stream.prover_fiat_shamir();
    // let indices = BrainFuckStark::<StarkConfig>::sample_indices(
    //     StarkConfig::SECURITY_LEVEL,
    //     indices_seed,
    //     65536,
    // );
    // assert_eq!(indices[0], 10);
    bfs.verify(&proof, &mut proof_stream).unwrap();
}

// #[test]
// fn zerofier() {
//     let n = 1usize << 4;
//     let offset = Fp::GENERATOR;
//     let root = Fp::get_root_of_unity(n as u64);

//     // x - 1
//     let poly = Univariate::new(vec![-Fp::one(), Fp::one()]);
//     let mut coefficients = poly.scale(offset).coefficients;
//     coefficients.resize(n, Fp::zero());

//     let eval1 = number_theory_transform(&coefficients);
//     let eval2 = (0..n)
//         .map(|i| offset * root.pow([i as u64]) - Fp::one())
//         .collect::<Vec<Fp>>();

//     assert_eq!(eval1, eval2);
// }
