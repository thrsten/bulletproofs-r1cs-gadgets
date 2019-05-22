extern crate rand;
extern crate curve25519_dalek;
extern crate merlin;
extern crate bulletproofs;

use curve25519_dalek::scalar::Scalar;
use bulletproofs::r1cs::{ConstraintSystem, R1CSError, R1CSProof, Variable, Prover, Verifier};
use bulletproofs::{BulletproofGens, PedersenGens};
use merlin::Transcript;
use bulletproofs::r1cs::LinearCombination;

use crate::r1cs_utils::{AllocatedScalar, constrain_lc_with_scalar};
use crate::gadget_zero_nonzero::is_nonzero_gadget;
use crate::poseidon_constants::{MDS_ENTRIES, ROUND_CONSTS};
use crate::scalar_utils::get_scalar_from_hex;

// TODO: Add serialization with serde
pub struct PoseidonParams {
    pub width: usize,
    // Number of full SBox rounds in beginning
    pub full_rounds_beginning: usize,
    // Number of full SBox rounds in end
    pub full_rounds_end: usize,
    // Number of partial SBox rounds in beginning
    pub partial_rounds: usize,
    pub round_keys: Vec<Scalar>,
    pub MDS_matrix: Vec<Vec<Scalar>>
}

impl PoseidonParams {
    fn new(width: usize, full_rounds_beginning: usize, full_rounds_end: usize, partial_rounds: usize) -> PoseidonParams {
        let total_rounds = full_rounds_beginning + partial_rounds + full_rounds_end;
//        let round_constants = Self::gen_round_constants(width, total_rounds);
        let round_keys = Self::gen_round_keys(width, total_rounds);
        let matrix_2 = Self::gen_MDS_matrix(width);
        PoseidonParams {
            width,
            full_rounds_beginning,
            full_rounds_end,
            partial_rounds,
            round_keys,
            MDS_matrix: matrix_2
        }
    }

    // TODO: Write logic to generate correct round keys. Currently loading hardcoded constants.
    fn gen_round_keys(width: usize, total_rounds: usize) -> Vec<Scalar> {
        let cap = total_rounds * width;
        // vec![Scalar::one(); cap]
        if ROUND_CONSTS.len() < cap {
            panic!("Not enough round constants, need {}, found {}", cap, ROUND_CONSTS.len());
        }
        let mut rc = vec![];
        for i in 0..cap {
            // TODO: Remove unwrap, handle error
            rc.push(get_scalar_from_hex(ROUND_CONSTS[i]).unwrap());
        }
        rc
    }

    // TODO: Write logic to generate correct MDS matrix
    fn gen_MDS_matrix(width: usize) -> Vec<Vec<Scalar>> {
        // vec![vec![Scalar::one(); width]; width]
        if MDS_ENTRIES.len() != width {
            panic!("Incorrect width, only width {} is supported now", width);
        }
        let mut mds: Vec<Vec<Scalar>> = vec![vec![Scalar::zero(); width]; width];
        for i in 0..width {
            if MDS_ENTRIES[i].len() != width {
                panic!("Incorrect width, only width {} is supported now", width);
            }
            for j in 0..width {
                // TODO: Remove unwrap, handle error
                mds[i][j] = get_scalar_from_hex(MDS_ENTRIES[i][j]).unwrap()
            }
        }
        mds
    }
}

pub enum SboxType {
    Cube,
    Inverse
}

impl SboxType {
    fn apply_sbox(&self, elem: &Scalar) -> Scalar {
        match self {
            SboxType::Cube => (elem * elem) * elem,
            SboxType::Inverse => elem.invert()
        }
    }

    fn synthesize_sbox<CS: ConstraintSystem>(
        &self,
        cs: &mut CS,
        input_var: LinearCombination,
        round_key: Scalar
    ) -> Result<Variable, R1CSError> {
        match self {
            SboxType::Cube => synthesize_cube_sbox(cs, input_var, round_key),
            SboxType::Inverse => synthesize_inverse_sbox(cs, input_var, round_key),
            _ => Err(R1CSError::GadgetError {description: String::from("inverse not implemented")})
        }
    }
}

// Allocate variables in circuit and enforce constraints when Sbox as cube
fn synthesize_cube_sbox<CS: ConstraintSystem>(
    cs: &mut CS,
    input_var: LinearCombination,
    round_key: Scalar
) -> Result<Variable, R1CSError> {
    let inp_plus_const: LinearCombination = input_var + round_key;
    let (i, _, sqr) = cs.multiply(inp_plus_const.clone(), inp_plus_const);
    let (_, _, cube) = cs.multiply(sqr.into(), i.into());
    Ok(cube)
}

// Allocate variables in circuit and enforce constraints when Sbox as inverse
fn synthesize_inverse_sbox<CS: ConstraintSystem>(
    cs: &mut CS,
    input_var: LinearCombination,
    round_key: Scalar
) -> Result<Variable, R1CSError> {
    let inp_plus_const: LinearCombination = input_var + round_key;

    let val_l = cs.evaluate_lc(&inp_plus_const);
    let val_r = val_l.map(|l| {
        l.invert()
    });

    let (var_l, _) = cs.allocate_single(val_l)?;
    let (var_r, var_o) = cs.allocate_single(val_r)?;

    // Ensure `inp_plus_const` is not zero
    is_nonzero_gadget(
        cs,
        AllocatedScalar {
            variable: var_l,
            assignment: val_l
        },
        AllocatedScalar {
            variable: var_r,
            assignment: val_r
        }
    )?;

    // Constrain product of ``inp_plus_const` and its inverse to be 1.
    constrain_lc_with_scalar::<CS>(cs, var_o.unwrap().into(), &Scalar::one());

    Ok(var_r)
}

fn Poseidon_permutation(
    input: &[Scalar],
    params: &PoseidonParams,
    sbox: &SboxType
) -> Vec<Scalar>
{
    let width = params.width;
    assert_eq!(input.len(), width);

    let full_rounds_beginning = params.full_rounds_beginning;
    let partial_rounds = params.partial_rounds;
    let full_rounds_end = params.full_rounds_end;

    let mut current_state = input.to_owned();
    let mut current_state_temp = vec![Scalar::zero(); width];

    let mut round_keys_offset = 0;

    // full Sbox rounds
    for _ in 0..full_rounds_beginning {
        // Sbox layer
        for i in 0..width {
            current_state[i] += params.round_keys[round_keys_offset];
            current_state[i] = sbox.apply_sbox(&current_state[i]);
            round_keys_offset += 1;
        }

        // linear layer
        for j in 0..width {
            for i in 0..width {
                current_state_temp[i] += current_state[j] * params.MDS_matrix[i][j];
            }
        }

        // Output of this round becomes input to next round
        for i in 0..width {
            current_state[i] = current_state_temp[i];
            current_state_temp[i] = Scalar::zero();
        }
    }

    // middle partial Sbox rounds
    for _ in full_rounds_beginning..(full_rounds_beginning+partial_rounds) {
        for i in 0..width {
            current_state[i] += &params.round_keys[round_keys_offset];
            round_keys_offset += 1;
        }

        // partial Sbox layer, apply Sbox to only 1 element of the state.
        // Here the last one is chosen but the choice is arbitrary.
        current_state[width-1] = sbox.apply_sbox(&current_state[width-1]);

        // linear layer
        for j in 0..width {
            for i in 0..width {
                current_state_temp[i] += current_state[j] * params.MDS_matrix[i][j];
            }
        }

        // Output of this round becomes input to next round
        for i in 0..width {
            current_state[i] = current_state_temp[i];
            current_state_temp[i] = Scalar::zero();
        }
    }

    // last full Sbox rounds
    for _ in full_rounds_beginning+partial_rounds..(full_rounds_beginning+partial_rounds+full_rounds_end) {
        // Sbox layer
        for i in 0..width {
            current_state[i] += params.round_keys[round_keys_offset];
            current_state[i] = sbox.apply_sbox(&current_state[i]);
            round_keys_offset += 1;
        }

        // linear layer
        for j in 0..width {
            for i in 0..width {
                current_state_temp[i] += current_state[j] * params.MDS_matrix[i][j];
            }
        }

        // Output of this round becomes input to next round
        for i in 0..width {
            current_state[i] = current_state_temp[i];
            current_state_temp[i] = Scalar::zero();
        }
    }

    // Finally the current_state becomes the output
    current_state
}

/// 2:1 (2 inputs, 1 output) hash from the permutation by passing the first input as zero, 2 of the next 4 as non-zero and rest zero. Choose one of the outputs.
pub fn Poseidon_hash_2(xl: Scalar, xr: Scalar, params: &PoseidonParams, sbox: &SboxType) -> Scalar {
    // Only 2 inputs to the permutation are set to the input of this hash function, rest are 0. Always keep the 1st input as 0

    let input = vec![
        Scalar::zero(),
        xl,
        xr,
        Scalar::zero(),
        Scalar::zero(),
        Scalar::zero()
    ];

    // Never take the first input
    Poseidon_permutation(&input, params, sbox)[1]
}

fn apply_linear_layer(
    width: usize,
    sbox_outs: Vec<LinearCombination>,
    next_inputs: &mut Vec<LinearCombination>,
    MDS_matrix: &Vec<Vec<Scalar>>,
) {
    for j in 0..width {
        for i in 0..width {
            next_inputs[i] = next_inputs[i].clone() + sbox_outs[j].clone() * MDS_matrix[i][j];
        }
    }
}

pub fn Poseidon_permutation_constraints<'a, CS: ConstraintSystem>(
    cs: &mut CS,
    input: Vec<AllocatedScalar>,
    params: &'a PoseidonParams,
    sbox_type: &SboxType
) -> Result<Vec<LinearCombination>, R1CSError> {
    let width = params.width;
    assert_eq!(input.len(), width);

    let mut input_vars: Vec<LinearCombination> = input.iter().map(|i|i.variable.into()).collect();

    let mut round_keys_offset = 0;

    let full_rounds_beginning = params.full_rounds_beginning;
    let partial_rounds = params.partial_rounds;
    let full_rounds_end = params.full_rounds_end;

    // ------------ First rounds with full SBox begin --------------------

    for k in 0..full_rounds_beginning {
        let mut sbox_outputs: Vec<LinearCombination> = vec![LinearCombination::default(); width];

        // Substitution (S-box) layer
        for i in 0..width {
            let round_key = params.round_keys[round_keys_offset];
            sbox_outputs[i] = sbox_type.synthesize_sbox(cs, input_vars[i].clone(), round_key.clone())?.into();

            round_keys_offset += 1;
        }

        let mut next_input_vars: Vec<LinearCombination> = vec![LinearCombination::default(); width];

        apply_linear_layer(width, sbox_outputs, &mut next_input_vars, &params.MDS_matrix);

        for i in 0..width {
            input_vars[i] = next_input_vars[i].to_owned();
        }
    }

    // ------------ First rounds with full SBox begin --------------------

    // ------------ Middle rounds with partial SBox begin --------------------

    for k in full_rounds_beginning..(full_rounds_beginning+partial_rounds) {
        let mut sbox_outputs: Vec<LinearCombination> = vec![LinearCombination::default(); width];

        // Substitution (S-box) layer
        for i in 0..width {
            let round_key = params.round_keys[round_keys_offset];

            // apply Sbox to only 1 element of the state.
            // Here the last one is chosen but the choice is arbitrary.
            if i == width-1 {
                sbox_outputs[i] = sbox_type.synthesize_sbox(cs, input_vars[i].clone(), round_key.clone())?.into();
            } else {
                sbox_outputs[i] = input_vars[i].clone() + LinearCombination::from(round_key);
            }

            round_keys_offset += 1;
        }

        // Linear layer

        let mut next_input_vars: Vec<LinearCombination> = vec![LinearCombination::default(); width];

        apply_linear_layer(width, sbox_outputs, &mut next_input_vars, &params.MDS_matrix);

        for i in 0..width {
            input_vars[i] = next_input_vars[i].to_owned();
        }
    }

    // ------------ Middle rounds with partial SBox end --------------------

    // ------------ Last rounds with full SBox begin --------------------

    for k in full_rounds_beginning+partial_rounds..(full_rounds_beginning+partial_rounds+full_rounds_end) {
        let mut sbox_outputs: Vec<LinearCombination> = vec![LinearCombination::default(); width];

        // Substitution (S-box) layer
        for i in 0..width {
            let round_key = params.round_keys[round_keys_offset];
            sbox_outputs[i] = sbox_type.synthesize_sbox(cs, input_vars[i].clone(), round_key.clone())?.into();

            round_keys_offset += 1;
        }

        // Linear layer

        let mut next_input_vars: Vec<LinearCombination> = vec![LinearCombination::default(); width];

        apply_linear_layer(width, sbox_outputs, &mut next_input_vars, &params.MDS_matrix);

        for i in 0..width {
            input_vars[i] = next_input_vars[i].to_owned();
        }
    }

    // ------------ Last rounds with full SBox end --------------------

    Ok(input_vars)
}


pub fn Poseidon_permutation_gadget<'a, CS: ConstraintSystem>(
    cs: &mut CS,
    input: Vec<AllocatedScalar>,
    params: &'a PoseidonParams,
    sbox_type: &SboxType,
    output: &[Scalar]
) -> Result<(), R1CSError> {
    let width = params.width;
    assert_eq!(output.len(), width);

    let permutation_output = Poseidon_permutation_constraints::<CS>(cs, input, params, sbox_type)?;

    for i in 0..width {
        constrain_lc_with_scalar::<CS>(cs, permutation_output[i].to_owned(), &output[i]);
    }

    Ok(())
}

pub fn Poseidon_hash_2_constraints<'a, CS: ConstraintSystem>(
    cs: &mut CS,
    xl: AllocatedScalar,
    xr: AllocatedScalar,
    zeros: Vec<AllocatedScalar>,
    params: &'a PoseidonParams,
    sbox_type: &SboxType,
) -> Result<LinearCombination, R1CSError> {
    let width = params.width;
    // Only 2 inputs to the permutation are set to the input of this hash function.
    assert_eq!(zeros.len(), width-2);

    // Always keep the 1st input as 0
    let mut inputs = vec![zeros[0]];
    inputs.push(xl);
    inputs.push(xr);

    // zeros correspond to committed variables with value and randomness both 0
    for i in 1..zeros.len() {
        inputs.push(zeros[i]);
    }
    let permutation_output = Poseidon_permutation_constraints::<CS>(cs, inputs, params, sbox_type)?;
    Ok(permutation_output[1].to_owned())
}

pub fn Poseidon_hash_2_gadget<'a, CS: ConstraintSystem>(
    cs: &mut CS,
    xl: AllocatedScalar,
    xr: AllocatedScalar,
    zeros: Vec<AllocatedScalar>,
    params: &'a PoseidonParams,
    sbox_type: &SboxType,
    output: &Scalar
) -> Result<(), R1CSError> {

    let hash = Poseidon_hash_2_constraints::<CS>(cs, xl, xr, zeros, params, sbox_type)?;

    constrain_lc_with_scalar::<CS>(cs, hash, output);

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    // For benchmarking
    use std::time::{Duration, Instant};
    use rand::SeedableRng;
    use super::rand::rngs::StdRng;

    fn poseidon_perm(sbox_type: &SboxType, transcript_label: &'static [u8]) {
        let mut test_rng: StdRng = SeedableRng::from_seed([24u8; 32]);
        let width = 6;
        let (full_b, full_e) = (4, 4);
        let partial_rounds = 140;
        let s_params = PoseidonParams::new(width, full_b, full_e, partial_rounds);
        let total_rounds = full_b + full_e + partial_rounds;

        let input = (0..width).map(|_| Scalar::random(&mut test_rng)).collect::<Vec<_>>();
        let expected_output = Poseidon_permutation(&input, &s_params, sbox_type);

        /*println!("Input:\n");
        println!("{:?}", &input);
        println!("Expected output:\n");
        println!("{:?}", &expected_output);*/

        let pc_gens = PedersenGens::default();
        let bp_gens = BulletproofGens::new(2048, 1);

        println!("Proving");
        let (proof, commitments) = {
            let mut prover_transcript = Transcript::new(transcript_label);
            let mut prover = Prover::new(&pc_gens, &mut prover_transcript);

            let mut comms = vec![];
            let mut allocs = vec![];

            for i in 0..width {
                let (com, var) = prover.commit(input[i].clone(), Scalar::random(&mut test_rng));
                comms.push(com);
                allocs.push(AllocatedScalar {
                    variable: var,
                    assignment: Some(input[i]),
                });
            }

            assert!(Poseidon_permutation_gadget(&mut prover,
                                                allocs,
                                                &s_params,
                                                sbox_type,
                                                &expected_output).is_ok());

            println!("For Poseidon permutation rounds {}, no of constraints is {}", total_rounds, &prover.num_constraints());

            let proof = prover.prove(&bp_gens).unwrap();
            (proof, comms)
        };

        println!("Verifying");

        let mut verifier_transcript = Transcript::new(transcript_label);
        let mut verifier = Verifier::new(&mut verifier_transcript);
        let mut allocs = vec![];
        for i in 0..width {
            let v = verifier.commit(commitments[i]);
            allocs.push(AllocatedScalar {
                variable: v,
                assignment: None,
            });
        }
        assert!(Poseidon_permutation_gadget(&mut verifier,
                                            allocs,
                                            &s_params,
                                            sbox_type,
                                            &expected_output).is_ok());

        assert!(verifier.verify(&proof, &pc_gens, &bp_gens).is_ok());
    }

    fn poseidon_hash(sbox_type: &SboxType, transcript_label: &'static [u8]) {
        let mut test_rng: StdRng = SeedableRng::from_seed([24u8; 32]);
        let width = 6;
        let (full_b, full_e) = (4, 4);
        let partial_rounds = 140;
        let s_params = PoseidonParams::new(width, full_b, full_e, partial_rounds);
        let total_rounds = full_b + full_e + partial_rounds;

        let xl = Scalar::random(&mut test_rng);
        let xr = Scalar::random(&mut test_rng);
        let expected_output = Poseidon_hash_2(xl, xr, &s_params, sbox_type);

        let pc_gens = PedersenGens::default();
        let bp_gens = BulletproofGens::new(2048, 1);

        println!("Proving");
        let (proof, commitments) = {
            let mut prover_transcript = Transcript::new(transcript_label);
            let mut prover = Prover::new(&pc_gens, &mut prover_transcript);

            let mut comms = vec![];
            let mut zero_allocs = vec![];

            let (com_l, var_l) = prover.commit(xl.clone(), Scalar::random(&mut test_rng));
            comms.push(com_l);
            let l_alloc = AllocatedScalar {
                variable: var_l,
                assignment: Some(xl),
            };

            let (com_r, var_r) = prover.commit(xr.clone(), Scalar::random(&mut test_rng));
            comms.push(com_r);
            let r_alloc = AllocatedScalar {
                variable: var_r,
                assignment: Some(xr),
            };

            // Commit to 0 with randomness 0 for the rest of the elements of width
            for _ in 2..width {
                let (_, var) = prover.commit(Scalar::zero(), Scalar::zero());
                zero_allocs.push(AllocatedScalar {
                    variable: var,
                    assignment: Some(Scalar::zero()),
                });
            }

            assert!(Poseidon_hash_2_gadget(&mut prover,
                                           l_alloc,
                                           r_alloc,
                                           zero_allocs,
                                           &s_params,
                                           sbox_type,
                                           &expected_output).is_ok());

            println!("For Poseidon hash rounds {}, no of constraints is {}", total_rounds, &prover.num_constraints());

            let proof = prover.prove(&bp_gens).unwrap();
            (proof, comms)
        };

        println!("Verifying");

        let mut verifier_transcript = Transcript::new(transcript_label);
        let mut verifier = Verifier::new(&mut verifier_transcript);
        let mut zero_allocs = vec![];
        let lv = verifier.commit(commitments[0]);
        let rv = verifier.commit(commitments[1]);
        let l_alloc = AllocatedScalar {
            variable: lv,
            assignment: None,
        };
        let r_alloc = AllocatedScalar {
            variable: rv,
            assignment: None,
        };

        // Commitment to 0 with blinding as 0
        let zero_comm = pc_gens.commit(Scalar::zero(), Scalar::zero()).compress();

        for i in 2..width {
            let v = verifier.commit(zero_comm.clone());
            zero_allocs.push(AllocatedScalar {
                variable: v,
                assignment: None,
            });
        }
        assert!(Poseidon_hash_2_gadget(&mut verifier,
                                       l_alloc,
                                       r_alloc,
                                       zero_allocs,
                                       &s_params,
                                       sbox_type,
                                       &expected_output).is_ok());

        assert!(verifier.verify(&proof, &pc_gens, &bp_gens).is_ok());
    }

    #[test]
    fn test_poseidon_perm_cube_sbox() {
        poseidon_perm(&SboxType::Cube, b"Poseidon_perm_cube");
    }

    #[test]
    fn test_poseidon_perm_inverse_sbox() {
        poseidon_perm(&SboxType::Inverse, b"Poseidon_perm_inverse");
    }

    #[test]
    fn test_poseidon_hash_cube_sbox() {
        poseidon_hash(&SboxType::Cube, b"Poseidon_hash_cube");
    }

    #[test]
    fn test_poseidon_hash_inverse_sbox() {
        poseidon_hash(&SboxType::Inverse, b"Poseidon_hash_inverse");
    }
}