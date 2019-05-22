Creating zero knowledge proofs using the [Bulletproofs implementation](https://github.com/dalek-cryptography/bulletproofs) from dalek-cryptography.
This repo contains several examples that show how various statements can be represented as arithmetic circuits which can be converted to R1CS. A [blog post](https://medium.com/coinmonks/zero-knowledge-proofs-using-bulletproofs-4a8e2579fc82) was written that explains the Bulletproofs API with several examples, though the API has changed slightly.   

  
## Circuits
1. [Prove a number is in certain range](src/gadget_bound_check.rs) 
2. [Prove value is non-zero](src/r1cs_utils.rs)
3. [Prove value is not equal to a given value](src/gadget_not_equals.rs)
4. Proof of set membership, 2 implementations [1](src/gadget_set_membership.rs), [2](src/gadget_set_membership_1.rs)
5. [Proof of set non-membership](src/gadget_set_non_membership.rs)
6. [Proof knowledge of preimage given image of MiMC hash function](src/gadget_mimc.rs)
7. [Proof of knowledge of leaf in a sparse merkle tree. Uses MiMC hash function.](src/gadget_vsmt.rs)
8. [Poseidon permutation and a 2:1 hash function based on it](src/gadget_poseidon.rs). 2 kinds of S-boxes, cube and inverse. Described in [this paper](https://eprint.iacr.org/2019/458). 
The parameters are generated using a sage worksheet supplied by Dmitry Khovratovich and a Jupyter notebook for that worksheet is present in the repo called Poseidon_Ristretto.ipynb. 

## Building
This project uses a slightly modified implementation of Bulletproofs's `develop` branch. The difference is addition of the method `num_constraints` to `Prover` 
to return the number of constraints and addition of 2 new methods in constraint system   
1. `evaluate_lc`: to evaluate an linear constraint 
2. `allocate_single`: to return output variable when allocating right multiplier.   

Use the nightly compiler to run tests like   
`cargo +nightly test --all-features`