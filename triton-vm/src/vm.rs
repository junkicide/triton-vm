use ndarray::Array2;
use ndarray::Axis;

use triton_opcodes::program::Program;
use twenty_first::shared_math::b_field_element::BFieldElement;
use twenty_first::shared_math::b_field_element::BFIELD_ZERO;
use twenty_first::shared_math::rescue_prime_regular::NUM_ROUNDS;
use twenty_first::shared_math::rescue_prime_regular::ROUND_CONSTANTS;
use twenty_first::shared_math::rescue_prime_regular::STATE_SIZE;

use crate::state::VMOutput;
use crate::state::VMState;
use crate::table::hash_table;
use crate::table::hash_table::NUM_ROUND_CONSTANTS;
use crate::table::processor_table;
use crate::table::table_column::BaseTableColumn;
use crate::table::table_column::HashBaseTableColumn::CONSTANT0A;
use crate::table::table_column::HashBaseTableColumn::ROUNDNUMBER;
use crate::table::table_column::HashBaseTableColumn::STATE0;

/// Simulate (execute) a `Program` and record every state transition. Returns an
/// `AlgebraicExecutionTrace` recording every intermediate state of the processor and all co-
/// processors.
///
/// On premature termination of the VM, returns the `AlgebraicExecutionTrace` for the execution
/// up to the point of failure.
pub fn simulate(
    program: &Program,
    mut stdin: Vec<BFieldElement>,
    mut secret_in: Vec<BFieldElement>,
) -> (
    AlgebraicExecutionTrace,
    Vec<BFieldElement>,
    Option<anyhow::Error>,
) {
    let mut aet = AlgebraicExecutionTrace::default();
    let mut state = VMState::new(program);
    // record initial state
    aet.processor_matrix
        .push_row(state.to_processor_row().view())
        .expect("shapes must be identical");

    let mut stdout = vec![];
    while !state.is_complete() {
        let vm_output = match state.step_mut(&mut stdin, &mut secret_in) {
            Err(err) => return (aet, stdout, Some(err)),
            Ok(vm_output) => vm_output,
        };

        match vm_output {
            Some(VMOutput::XlixTrace(hash_trace)) => aet.append_hash_trace(*hash_trace),
            Some(VMOutput::WriteOutputSymbol(written_word)) => stdout.push(written_word),
            None => (),
        }
        // Record next, to be executed state.
        aet.processor_matrix
            .push_row(state.to_processor_row().view())
            .expect("shapes must be identical");
    }

    (aet, stdout, None)
}

/// Wrapper around `.simulate_with_input()` and thus also around
/// `.simulate()` for convenience when neither explicit nor non-
/// deterministic input is provided. Behavior is the same as that
/// of `.simulate_with_input()`
pub fn simulate_no_input(
    program: &Program,
) -> (
    AlgebraicExecutionTrace,
    Vec<BFieldElement>,
    Option<anyhow::Error>,
) {
    simulate(program, vec![], vec![])
}

pub fn run(
    program: &Program,
    mut stdin: Vec<BFieldElement>,
    mut secret_in: Vec<BFieldElement>,
) -> (Vec<VMState>, Vec<BFieldElement>, Option<anyhow::Error>) {
    let mut states = vec![VMState::new(program)];
    let mut current_state = states.last().unwrap();

    let mut stdout = vec![];
    while !current_state.is_complete() {
        let step = current_state.step(&mut stdin, &mut secret_in);
        let (next_state, vm_output) = match step {
            Err(err) => {
                println!("Encountered an error when running VM.");
                return (states, stdout, Some(err));
            }
            Ok((next_state, vm_output)) => (next_state, vm_output),
        };

        if let Some(VMOutput::WriteOutputSymbol(written_word)) = vm_output {
            stdout.push(written_word);
        }

        states.push(next_state);
        current_state = states.last().unwrap();
    }

    (states, stdout, None)
}

#[derive(Debug, Clone)]
pub struct AlgebraicExecutionTrace {
    pub processor_matrix: Array2<BFieldElement>,
    pub hash_matrix: Array2<BFieldElement>,
}

impl Default for AlgebraicExecutionTrace {
    fn default() -> Self {
        Self {
            processor_matrix: Array2::default([0, processor_table::BASE_WIDTH]),
            hash_matrix: Array2::default([0, hash_table::BASE_WIDTH]),
        }
    }
}

impl AlgebraicExecutionTrace {
    pub fn append_hash_trace(&mut self, hash_trace: [[BFieldElement; STATE_SIZE]; NUM_ROUNDS + 1]) {
        let mut hash_matrix_addendum = Array2::default([NUM_ROUNDS + 1, hash_table::BASE_WIDTH]);
        for (row_idx, mut row) in hash_matrix_addendum.rows_mut().into_iter().enumerate() {
            let round_number = row_idx + 1;
            let trace_row = hash_trace[row_idx];
            let round_constants = Self::rescue_xlix_round_constants_by_round_number(round_number);
            row[ROUNDNUMBER.base_table_index()] = BFieldElement::from(row_idx as u64 + 1);
            for st_idx in 0..STATE_SIZE {
                row[STATE0.base_table_index() + st_idx] = trace_row[st_idx];
            }
            for rc_idx in 0..NUM_ROUND_CONSTANTS {
                row[CONSTANT0A.base_table_index() + rc_idx] = round_constants[rc_idx];
            }
        }
        self.hash_matrix
            .append(Axis(0), hash_matrix_addendum.view())
            .expect("shapes must be identical");
    }

    /// The 2·STATE_SIZE (= NUM_ROUND_CONSTANTS) round constants for round `round_number`.
    /// Of note:
    /// - Round index 0 indicates a padding row – all constants are zero.
    /// - Round index 9 indicates an output row – all constants are zero.
    pub fn rescue_xlix_round_constants_by_round_number(
        round_number: usize,
    ) -> [BFieldElement; NUM_ROUND_CONSTANTS] {
        match round_number {
            i if i == 0 || i == NUM_ROUNDS + 1 => [BFIELD_ZERO; NUM_ROUND_CONSTANTS],
            i if i <= NUM_ROUNDS => ROUND_CONSTANTS
                [NUM_ROUND_CONSTANTS * (i - 1)..NUM_ROUND_CONSTANTS * i]
                .try_into()
                .unwrap(),
            _ => panic!("Round with number {round_number} does not have round constants."),
        }
    }
}

#[cfg(test)]
pub mod triton_vm_tests {
    use std::ops::BitAnd;
    use std::ops::BitXor;

    use itertools::Itertools;
    use ndarray::Array1;
    use ndarray::ArrayView1;
    use num_traits::One;
    use num_traits::Zero;
    use rand::rngs::ThreadRng;
    use rand::Rng;
    use rand::RngCore;
    use twenty_first::shared_math::other::random_elements;
    use twenty_first::shared_math::rescue_prime_regular::RescuePrimeRegular;
    use twenty_first::shared_math::traits::FiniteField;

    use crate::shared_tests::SourceCodeAndInput;
    use crate::table::processor_table::ProcessorMatrixRow;

    use super::*;

    fn pretty_print_array_view<FF: FiniteField>(array: ArrayView1<FF>) -> String {
        array
            .iter()
            .map(|ff| format!("{ff}"))
            .collect_vec()
            .join(", ")
    }

    pub const GCD_X_Y: &str = "
        read_io  // _ a
        read_io  // _ a b
        dup1     // _ a b a
        dup1     // _ a b a b
        lt       // _ a b b<a
        skiz     // _ a b
            swap1  // _ d n where n > d

        // ---
        loop_cond:
        dup1
        push 0 
        eq 
        skiz 
            call terminate  // _ d n where d != 0
        dup1   // _ d n d
        dup1   // _ d n d n
        div    // _ d n q r
        swap2  // _ d r q n
        pop    // _ d r q
        pop    // _ d r
        swap1  // _ r d
        call loop_cond
        // ---
        
        terminate:
            // _ d n where d == 0
            write_io // _ d
            halt
        ";

    #[test]
    fn initialise_table_test() {
        let code = GCD_X_Y;
        let program = Program::from_code(code).unwrap();

        let stdin = vec![BFieldElement::new(42), BFieldElement::new(56)];

        let (aet, stdout, err) = simulate(&program, stdin, vec![]);

        println!(
            "VM output: [{}]",
            pretty_print_array_view(Array1::from(stdout).view())
        );

        if let Some(e) = err {
            panic!("Execution failed: {e}");
        }
        for row in aet.processor_matrix.rows() {
            println!("{}", ProcessorMatrixRow { row });
        }
    }

    #[test]
    fn initialise_table_42_test() {
        // 1. Execute program
        let code = "
        push 5
        push 18446744069414584320
        add
        halt
    ";
        let program = Program::from_code(code).unwrap();

        println!("{}", program);

        let (aet, _, err) = simulate_no_input(&program);

        println!("{:?}", err);
        for row in aet.processor_matrix.rows() {
            println!("{}", ProcessorMatrixRow { row });
        }
    }

    #[test]
    fn simulate_tvm_gcd_test() {
        let code = GCD_X_Y;
        let program = Program::from_code(code).unwrap();

        let stdin = vec![42_u64.into(), 56_u64.into()];
        let (_, stdout, err) = simulate(&program, stdin, vec![]);

        let stdout = Array1::from(stdout);
        println!("VM output: [{}]", pretty_print_array_view(stdout.view()));

        if let Some(e) = err {
            panic!("Execution failed: {e}");
        }

        let expected_symbol = BFieldElement::new(14);
        let computed_symbol = stdout[0];

        assert_eq!(expected_symbol, computed_symbol);
    }

    pub fn test_hash_nop_nop_lt() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input("hash nop hash nop nop hash push 3 push 2 lt assert halt")
    }

    pub fn test_program_for_halt() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input("halt")
    }

    pub fn test_program_for_push_pop_dup_swap_nop() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input(
            "push 1 push 2 pop assert \
            push 1 dup0 assert assert \
            push 1 push 2 swap1 assert pop \
            nop nop nop halt",
        )
    }

    pub fn test_program_for_divine() -> SourceCodeAndInput {
        SourceCodeAndInput {
            source_code: "divine assert halt".to_string(),
            input: vec![],
            secret_input: vec![BFieldElement::one()],
        }
    }

    pub fn test_program_for_skiz() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input("push 1 skiz push 0 skiz assert push 1 skiz halt")
    }

    pub fn test_program_for_call_recurse_return() -> SourceCodeAndInput {
        let source_code = "push 2 call label halt label: push -1 add dup0 skiz recurse return";
        SourceCodeAndInput::without_input(source_code)
    }

    pub fn test_program_for_write_mem_read_mem() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input("push 2 push 1 write_mem pop push 0 read_mem assert halt")
    }

    pub fn test_program_for_hash() -> SourceCodeAndInput {
        let source_code =
            "push 0 push 0 push 0 push 1 push 2 push 3 hash pop pop pop pop pop read_io eq assert halt";
        let mut hash_input = [BFieldElement::zero(); 10];
        hash_input[0] = BFieldElement::new(3);
        hash_input[1] = BFieldElement::new(2);
        hash_input[2] = BFieldElement::new(1);
        let digest = RescuePrimeRegular::hash_10(&hash_input);
        SourceCodeAndInput {
            source_code: source_code.to_string(),
            input: vec![digest.to_vec()[0]],
            secret_input: vec![],
        }
    }

    pub fn test_program_for_divine_sibling_noswitch() -> SourceCodeAndInput {
        let source_code = "
            push 3 \
            push 4 push 2 push 2 push 2 push 1 \
            push 5679457 push 1337 push 345887 push -234578456 push 23657565 \
            divine_sibling \
            push 1 add assert assert assert assert assert \
            assert \
            push -1 add assert \
            push -1 add assert \
            push -1 add assert \
            push -3 add assert \
            assert halt ";
        let one = BFieldElement::one();
        let zero = BFieldElement::zero();
        SourceCodeAndInput {
            source_code: source_code.to_string(),
            input: vec![],
            secret_input: vec![one, one, one, one, zero],
        }
    }

    pub fn test_program_for_divine_sibling_switch() -> SourceCodeAndInput {
        let source_code = "
            push 2 \
            push 4 push 2 push 2 push 2 push 1 \
            push 5679457 push 1337 push 345887 push -234578456 push 23657565 \
            divine_sibling \
            assert \
            push -1 add assert \
            push -1 add assert \
            push -1 add assert \
            push -3 add assert \
            push 1 add assert assert assert assert assert \
            assert halt ";
        let one = BFieldElement::one();
        let zero = BFieldElement::zero();
        SourceCodeAndInput {
            source_code: source_code.to_string(),
            input: vec![],
            secret_input: vec![one, one, one, one, zero],
        }
    }

    pub fn test_program_for_assert_vector() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input(
            "push 1 push 2 push 3 push 4 push 5 \
             push 1 push 2 push 3 push 4 push 5 \
             assert_vector halt",
        )
    }

    pub fn test_program_for_eq_vector() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input(
            "push 1 push 2 push 3 push 4 push 5 \
             push 1 push 2 push 3 push 4 push 5 \
             eq_vector halt",
        )
    }

    pub fn property_based_test_program_for_assert_vector() -> SourceCodeAndInput {
        let mut rng = ThreadRng::default();
        let st0 = rng.gen_range(0..BFieldElement::QUOTIENT);
        let st1 = rng.gen_range(0..BFieldElement::QUOTIENT);
        let st2 = rng.gen_range(0..BFieldElement::QUOTIENT);
        let st3 = rng.gen_range(0..BFieldElement::QUOTIENT);
        let st4 = rng.gen_range(0..BFieldElement::QUOTIENT);

        let source_code = format!(
            "push {} push {} push {} push {} push {} \
            read_io read_io read_io read_io read_io \
            assert_vector halt",
            st4, st3, st2, st1, st0,
        );

        SourceCodeAndInput {
            source_code,
            input: vec![st4.into(), st3.into(), st2.into(), st1.into(), st0.into()],
            secret_input: vec![],
        }
    }

    pub fn test_program_for_add_mul_invert() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input(
            "push 2 push -1 add assert \
            push -1 push -1 mul assert \
            push 3 dup0 invert mul assert \
            halt",
        )
    }

    pub fn test_program_for_instruction_split() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input("push -1 split swap1 lt assert halt ")
    }

    pub fn property_based_test_program_for_split() -> SourceCodeAndInput {
        let mut rng = ThreadRng::default();
        let st0 = rng.next_u64() % BFieldElement::QUOTIENT;
        let hi = st0 >> 32;
        let lo = st0 & 0xffff_ffff;

        let source_code = format!(
            "push {} split read_io eq assert read_io eq assert halt",
            st0
        );

        SourceCodeAndInput {
            source_code,
            input: vec![hi.into(), lo.into()],
            secret_input: vec![],
        }
    }

    pub fn test_program_for_eq() -> SourceCodeAndInput {
        SourceCodeAndInput {
            source_code: "read_io divine eq assert halt".to_string(),
            input: vec![BFieldElement::new(42)],
            secret_input: vec![BFieldElement::new(42)],
        }
    }

    pub fn property_based_test_program_for_eq() -> SourceCodeAndInput {
        let mut rng = ThreadRng::default();
        let st0 = rng.next_u64() % BFieldElement::QUOTIENT;

        let source_code = format!(
            "push {} dup0 read_io eq assert dup0 divine eq assert halt",
            st0
        );

        SourceCodeAndInput {
            source_code,
            input: vec![st0.into()],
            secret_input: vec![st0.into()],
        }
    }

    pub fn test_program_for_lsb() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input("push 3 lsb assert assert halt")
    }

    pub fn property_based_test_program_for_lsb() -> SourceCodeAndInput {
        let mut rng = ThreadRng::default();
        let st0 = rng.next_u32();
        let lsb = st0 % 2;
        let st0_shift_right = st0 >> 1;

        let source_code = format!("push {} lsb read_io eq assert read_io eq assert halt", st0);

        SourceCodeAndInput {
            source_code,
            input: vec![lsb.into(), st0_shift_right.into()],
            secret_input: vec![],
        }
    }

    pub fn test_program_for_lt() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input("push 5 push 2 lt assert halt")
    }

    pub fn property_based_test_program_for_lt() -> SourceCodeAndInput {
        let mut rng = ThreadRng::default();
        let st1 = rng.next_u32();
        let st0 = rng.next_u32();
        let result = if st0 < st1 {
            1_u64.into()
        } else {
            0_u64.into()
        };

        let source_code = format!("push {} push {} lt read_io eq assert halt", st1, st0);

        SourceCodeAndInput {
            source_code,
            input: vec![result],
            secret_input: vec![],
        }
    }

    pub fn test_program_for_and() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input("push 5 push 3 and assert halt")
    }

    pub fn property_based_test_program_for_and() -> SourceCodeAndInput {
        let mut rng = ThreadRng::default();
        let st1 = rng.next_u32();
        let st0 = rng.next_u32();
        let result = st0.bitand(st1);

        let source_code = format!("push {} push {} and read_io eq assert halt", st1, st0);

        SourceCodeAndInput {
            source_code,
            input: vec![result.into()],
            secret_input: vec![],
        }
    }

    pub fn test_program_for_xor() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input("push 7 push 6 xor assert halt")
    }

    pub fn property_based_test_program_for_xor() -> SourceCodeAndInput {
        let mut rng = ThreadRng::default();
        let st1 = rng.next_u32();
        let st0 = rng.next_u32();
        let result = st0.bitxor(st1);

        let source_code = format!("push {} push {} xor read_io eq assert halt", st1, st0);

        SourceCodeAndInput {
            source_code,
            input: vec![result.into()],
            secret_input: vec![],
        }
    }

    pub fn test_program_for_reverse() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input("push 2147483648 reverse assert halt")
    }

    pub fn property_based_test_program_for_reverse() -> SourceCodeAndInput {
        let mut rng = ThreadRng::default();
        let st0 = rng.next_u32();
        let st0_rev = st0.reverse_bits().into();

        let source_code = format!("push {} reverse read_io eq assert halt", st0);

        SourceCodeAndInput {
            source_code,
            input: vec![st0_rev],
            secret_input: vec![],
        }
    }

    pub fn test_program_for_lte() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input("push 5 push 2 lte assert halt")
    }

    pub fn property_based_test_program_for_lte() -> SourceCodeAndInput {
        let mut rng = ThreadRng::default();
        let st1 = rng.next_u32();
        let st0 = rng.next_u32();
        let result = if st0 <= st1 {
            1_u64.into()
        } else {
            0_u64.into()
        };

        let source_code = format!("push {} push {} lte read_io eq assert halt", st1, st0);

        SourceCodeAndInput {
            source_code,
            input: vec![result],
            secret_input: vec![],
        }
    }

    pub fn test_program_for_div() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input("push 2 push 3 div assert assert halt")
    }

    pub fn property_based_test_program_for_div() -> SourceCodeAndInput {
        let mut rng = ThreadRng::default();
        let denominator = rng.next_u32();
        let numerator = rng.next_u32();
        let quotient = numerator / denominator;
        let remainder = numerator % denominator;

        let source_code = format!(
            "push {} push {} div read_io eq assert read_io eq assert halt",
            denominator, numerator
        );

        SourceCodeAndInput {
            source_code,
            input: vec![remainder.into(), quotient.into()],
            secret_input: vec![],
        }
    }

    pub fn property_based_test_program_for_is_u32() -> SourceCodeAndInput {
        let mut rng = ThreadRng::default();
        let st0 = rng.next_u32();

        let source_code = format!("push {} is_u32 halt", st0);

        SourceCodeAndInput::without_input(&source_code)
    }

    pub fn property_based_test_program_for_random_ram_access() -> SourceCodeAndInput {
        let mut rng = ThreadRng::default();
        let num_memory_accesses = rng.gen_range(10..50);
        let memory_addresses: Vec<BFieldElement> = random_elements(num_memory_accesses);
        let mut memory_values: Vec<BFieldElement> = random_elements(num_memory_accesses);
        let mut source_code = String::new();

        // Read some memory before first write to ensure that the memory is initialized with 0s.
        // Not all addresses are read to have different access patterns:
        // - Some addresses are read before written to.
        // - Other addresses are written to before read.
        for memory_address in memory_addresses.iter().take(num_memory_accesses / 4) {
            source_code.push_str(&format!(
                "push {memory_address} push 0 read_mem push 0 eq assert pop "
            ));
        }

        // Write everything to RAM.
        for (memory_address, memory_value) in memory_addresses.iter().zip_eq(memory_values.iter()) {
            source_code.push_str(&format!(
                "push {memory_address} push {memory_value} write_mem pop pop "
            ));
        }

        // Read back in random order and check that the values did not change.
        // For repeated sampling from the same range, better performance can be achieved by using
        // `Uniform`. However, this is a test, and not very many samples – it's fine.
        let mut reading_permutation = (0..num_memory_accesses).collect_vec();
        for i in 0..num_memory_accesses {
            let j = rng.gen_range(0..num_memory_accesses);
            reading_permutation.swap(i, j);
        }
        for idx in reading_permutation {
            let memory_address = memory_addresses[idx];
            let memory_value = memory_values[idx];
            source_code.push_str(&format!(
                "push {memory_address} push 0 read_mem push {memory_value} eq assert pop "
            ));
        }

        // Overwrite half the values with new ones.
        let mut writing_permutation = (0..num_memory_accesses).collect_vec();
        for i in 0..num_memory_accesses {
            let j = rng.gen_range(0..num_memory_accesses);
            writing_permutation.swap(i, j);
        }
        for idx in 0..num_memory_accesses / 2 {
            let memory_address = memory_addresses[writing_permutation[idx]];
            let new_memory_value = rng.gen();
            memory_values[writing_permutation[idx]] = new_memory_value;
            source_code.push_str(&format!(
                "push {memory_address} push {new_memory_value} write_mem pop pop "
            ));
        }

        // Read back all, i.e., unchanged and overwritten values in (different from before) random
        // order and check that the values did not change.
        let mut reading_permutation = (0..num_memory_accesses).collect_vec();
        for i in 0..num_memory_accesses {
            let j = rng.gen_range(0..num_memory_accesses);
            reading_permutation.swap(i, j);
        }
        for idx in reading_permutation {
            let memory_address = memory_addresses[idx];
            let memory_value = memory_values[idx];
            source_code.push_str(&format!(
                "push {memory_address} push 0 read_mem push {memory_value} eq assert pop "
            ));
        }

        source_code.push_str("halt");
        SourceCodeAndInput::without_input(&source_code)
    }

    #[test]
    // Sanity check for the relatively complex property-based test for random RAM access.
    fn run_dont_prove_property_based_test_for_random_ram_access() {
        let source_code_and_input = property_based_test_program_for_random_ram_access();
        source_code_and_input.run();
    }

    #[test]
    #[should_panic(expected = "st0 must be 1.")]
    pub fn negative_property_is_u32_test() {
        let mut rng = ThreadRng::default();
        let st0 = (rng.next_u32() as u64) << 32;

        let source_code = format!("push {} is_u32 halt", st0);
        let program = SourceCodeAndInput::without_input(&source_code);
        let _ = program.run();
    }

    pub fn test_program_for_split() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input(
            "push -2 split push 4294967294 eq assert push 4294967295 eq assert \
             push -1 split push 4294967295 eq assert push 0 eq assert \
             push  0 split push 0 eq assert push 0 eq assert \
             push  1 split push 0 eq assert push 1 eq assert \
             push  2 split push 0 eq assert push 2 eq assert \
             push 4294967297 split assert assert \
             halt",
        )
    }

    pub fn test_program_for_split_assert() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input(
            "push -2 split_assert push 4294967294 eq assert push 4294967295 eq assert \
             push -1 split_assert push 4294967295 eq assert push 0 eq assert \
             push  0 split_assert push 0 eq assert push 0 eq assert \
             push  1 split_assert push 0 eq assert push 1 eq assert \
             push  2 split_assert push 0 eq assert push 2 eq assert \
             push 4294967297 split_assert assert assert \
             halt",
        )
    }

    pub fn test_program_for_xxadd() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input("push 5 push 6 push 7 push 8 push 9 push 10 xxadd halt")
    }

    pub fn test_program_for_xxmul() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input("push 5 push 6 push 7 push 8 push 9 push 10 xxmul halt")
    }

    pub fn test_program_for_xinvert() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input("push 5 push 6 push 7 xinvert halt")
    }

    pub fn test_program_for_xbmul() -> SourceCodeAndInput {
        SourceCodeAndInput::without_input("push 5 push 6 push 7 push 8 xbmul halt")
    }

    pub fn test_program_for_read_io_write_io() -> SourceCodeAndInput {
        SourceCodeAndInput {
            source_code: "read_io assert read_io read_io dup1 dup1 add write_io mul write_io halt"
                .to_string(),
            input: vec![1_u64.into(), 3_u64.into(), 14_u64.into()],
            secret_input: vec![],
        }
    }

    pub fn small_tasm_test_programs() -> Vec<SourceCodeAndInput> {
        vec![
            test_program_for_halt(),
            test_program_for_push_pop_dup_swap_nop(),
            test_program_for_divine(),
            test_program_for_skiz(),
            test_program_for_call_recurse_return(),
            test_program_for_write_mem_read_mem(),
            test_program_for_hash(),
            test_program_for_divine_sibling_noswitch(),
            test_program_for_divine_sibling_switch(),
            test_program_for_assert_vector(),
            test_program_for_eq_vector(),
            test_program_for_add_mul_invert(),
            test_program_for_eq(),
            test_program_for_lsb(),
            test_program_for_split(),
            test_program_for_xxadd(),
            test_program_for_xxmul(),
            test_program_for_xinvert(),
            test_program_for_xbmul(),
            test_program_for_read_io_write_io(),
        ]
    }

    pub fn property_based_test_programs() -> Vec<SourceCodeAndInput> {
        vec![
            property_based_test_program_for_assert_vector(),
            property_based_test_program_for_split(),
            property_based_test_program_for_eq(),
            property_based_test_program_for_lsb(),
            property_based_test_program_for_lt(),
            property_based_test_program_for_and(),
            property_based_test_program_for_xor(),
            property_based_test_program_for_reverse(),
            property_based_test_program_for_lte(),
            property_based_test_program_for_div(),
            property_based_test_program_for_is_u32(),
            property_based_test_program_for_random_ram_access(),
        ]
    }

    /// programs with a cycle count of 150 and upwards
    pub fn bigger_tasm_test_programs() -> Vec<SourceCodeAndInput> {
        vec![
            test_hash_nop_nop_lt(),
            test_program_for_instruction_split(),
            test_program_for_lt(),
            test_program_for_and(),
            test_program_for_xor(),
            test_program_for_reverse(),
            test_program_for_lte(),
            test_program_for_div(),
            test_program_for_split_assert(),
        ]
    }

    #[test]
    fn xxadd_test() {
        let stdin_words = vec![
            BFieldElement::new(2),
            BFieldElement::new(3),
            BFieldElement::new(5),
            BFieldElement::new(7),
            BFieldElement::new(11),
            BFieldElement::new(13),
        ];
        let xxadd_code = "
            read_io read_io read_io
            read_io read_io read_io
            xxadd
            swap2
            write_io write_io write_io
            halt
        ";
        let program = SourceCodeAndInput {
            source_code: xxadd_code.to_string(),
            input: stdin_words,
            secret_input: vec![],
        };

        let actual_stdout = program.run();
        let expected_stdout = vec![
            BFieldElement::new(9),
            BFieldElement::new(14),
            BFieldElement::new(18),
        ];

        assert_eq!(expected_stdout, actual_stdout);
    }

    #[test]
    fn xxmul_test() {
        let stdin_words = vec![
            BFieldElement::new(2),
            BFieldElement::new(3),
            BFieldElement::new(5),
            BFieldElement::new(7),
            BFieldElement::new(11),
            BFieldElement::new(13),
        ];
        let xxmul_code = "
            read_io read_io read_io
            read_io read_io read_io
            xxmul
            swap2
            write_io write_io write_io
            halt
        ";
        let program = SourceCodeAndInput {
            source_code: xxmul_code.to_string(),
            input: stdin_words,
            secret_input: vec![],
        };

        let actual_stdout = program.run();
        let expected_stdout = vec![
            BFieldElement::new(108),
            BFieldElement::new(123),
            BFieldElement::new(22),
        ];

        assert_eq!(expected_stdout, actual_stdout);
    }

    #[test]
    fn xinv_test() {
        let stdin_words = vec![
            BFieldElement::new(2),
            BFieldElement::new(3),
            BFieldElement::new(5),
        ];
        let xinv_code = "
            read_io read_io read_io
            dup2 dup2 dup2
            dup2 dup2 dup2
            xinvert xxmul
            swap2
            write_io write_io write_io
            xinvert
            swap2
            write_io write_io write_io
            halt";
        let program = SourceCodeAndInput {
            source_code: xinv_code.to_string(),
            input: stdin_words,
            secret_input: vec![],
        };

        let actual_stdout = program.run();
        let expected_stdout = vec![
            BFieldElement::zero(),
            BFieldElement::zero(),
            BFieldElement::one(),
            BFieldElement::new(16360893149904808002),
            BFieldElement::new(14209859389160351173),
            BFieldElement::new(4432433203958274678),
        ];

        assert_eq!(expected_stdout, actual_stdout);
    }

    #[test]
    fn xbmul_test() {
        let stdin_words = vec![
            BFieldElement::new(2),
            BFieldElement::new(3),
            BFieldElement::new(5),
            BFieldElement::new(7),
        ];
        let xbmul_code: &str = "
            read_io read_io read_io
            read_io
            xbmul
            swap2
            write_io write_io write_io
            halt";
        let program = SourceCodeAndInput {
            source_code: xbmul_code.to_string(),
            input: stdin_words,
            secret_input: vec![],
        };

        let actual_stdout = program.run();
        let expected_stdout = [14, 21, 35].map(BFieldElement::new).to_vec();

        assert_eq!(expected_stdout, actual_stdout);
    }

    #[test]
    fn pseudo_sub_test() {
        let actual_stdout =
            SourceCodeAndInput::without_input("push 7 push 19 sub write_io halt").run();
        let expected_stdout = vec![BFieldElement::new(12)];

        assert_eq!(expected_stdout, actual_stdout);
    }
}
