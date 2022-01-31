use halo2::{
    circuit::Chip,
    plonk::{
        Advice, Column, ConstraintSystem, Expression, Fixed, VirtualCells,
    },
    poly::Rotation,
};
use itertools::Itertools;
use pairing::arithmetic::FieldExt;
use std::marker::PhantomData;

use crate::{param::{HASH_WIDTH, IS_EXTENSION_ODD_KEY_LEN_POS, IS_EXTENSION_KEY_SHORT_POS, LAYOUT_OFFSET, IS_EXTENSION_NODE_POS, IS_EXTENSION_EVEN_KEY_LEN_POS, IS_EXTENSION_KEY_LONG_POS}, helpers::compute_rlc};

#[derive(Clone, Debug)]
pub(crate) struct ExtensionNodeKeyConfig {}

pub(crate) struct ExtensionNodeKeyChip<F> {
    config: ExtensionNodeKeyConfig,
    _marker: PhantomData<F>,
}

impl<F: FieldExt> ExtensionNodeKeyChip<F> {
    pub fn configure(
        meta: &mut ConstraintSystem<F>,
        q_not_first: Column<Fixed>,
        not_first_level: Column<Fixed>, // to avoid rotating back when in the first branch (for key rlc)
        is_branch_init: Column<Advice>,
        is_branch_child: Column<Advice>,
        is_last_branch_child: Column<Advice>,
        is_account_leaf_storage_codehash_c: Column<Advice>,
        s_rlp2: Column<Advice>,
        s_advices: [Column<Advice>; HASH_WIDTH],
        modified_node: Column<Advice>, // index of the modified node
        // sel1 and sel2 in branch init: denote whether it's the first or second nibble of the key byte
        // sel1 and sel2 in branch children: denote whether there is no leaf at is_modified (when value is added or deleted from trie)
        sel1: Column<Advice>,
        sel2: Column<Advice>,
        key_rlc: Column<Advice>, // used first for account address, then for storage key
        key_rlc_mult: Column<Advice>,
        r_table: Vec<Expression<F>>,
    ) -> ExtensionNodeKeyConfig {
        let config = ExtensionNodeKeyConfig {};
        let one = Expression::Constant(F::one());

        meta.create_gate("extension node key", |meta| {
            let q_not_first = meta.query_fixed(q_not_first, Rotation::cur());
            let not_first_level =
                meta.query_fixed(not_first_level, Rotation::cur());

            let mut constraints = vec![];

            let rot_into_branch_init = -18;
            // Could be used any rotation into previous branch, because key RLC is the same in all
            // branch children:
            let rot_into_prev_branch = rot_into_branch_init - 3;
            let c16 = Expression::Constant(F::from(16));

            let is_extension_node = meta.query_advice(
                s_advices[IS_EXTENSION_NODE_POS - LAYOUT_OFFSET],
                Rotation(rot_into_branch_init),
            );

            // NOTE: is_key_even and is_key_odd is for number of nibbles that
            // are compactly encoded.
            let is_key_even = meta.query_advice(
                s_advices[IS_EXTENSION_EVEN_KEY_LEN_POS - LAYOUT_OFFSET],
                Rotation(rot_into_branch_init),
            );
            let is_key_odd = meta.query_advice(
                s_advices[IS_EXTENSION_ODD_KEY_LEN_POS - LAYOUT_OFFSET],
                Rotation(rot_into_branch_init),
            );
            let is_short = meta.query_advice(
                s_advices[IS_EXTENSION_KEY_SHORT_POS - LAYOUT_OFFSET],
                Rotation(rot_into_branch_init),
            );
            let is_long = meta.query_advice(
                s_advices[IS_EXTENSION_KEY_LONG_POS - LAYOUT_OFFSET],
                Rotation(rot_into_branch_init),
            );

            let sel1 =
                meta.query_advice(sel1, Rotation(rot_into_branch_init));
            let sel2 =
                meta.query_advice(sel2, Rotation(rot_into_branch_init));

            // We are in extension row C, -18 brings us in the branch init row.
            // -19 is account leaf storage codehash when we are in the first storage proof level.
            let is_account_leaf_storage_codehash_prev = meta.query_advice(
                is_account_leaf_storage_codehash_c,
                Rotation(rot_into_branch_init-1),
            );

            let is_branch_init_prev =
                meta.query_advice(is_branch_init, Rotation::prev());
            let is_branch_child_prev =
                meta.query_advice(is_branch_child, Rotation::prev());
            let is_branch_child_cur =
                meta.query_advice(is_branch_child, Rotation::cur());

            // Any rotation that lands into branch children can be used:
            let modified_node_cur =
                meta.query_advice(modified_node, Rotation(-2));

            let is_extension_s_row =
                meta.query_advice(is_last_branch_child, Rotation(-1));
            let is_extension_c_row =
                meta.query_advice(is_last_branch_child, Rotation(-2));

            let key_rlc_prev = meta.query_advice(key_rlc, Rotation::prev());
            let key_rlc_prev_level = meta.query_advice(key_rlc, Rotation(rot_into_prev_branch));
            let key_rlc_cur = meta.query_advice(key_rlc, Rotation::cur());

            let key_rlc_mult_prev = meta.query_advice(key_rlc_mult, Rotation::prev());
            let key_rlc_mult_prev_level = meta.query_advice(key_rlc_mult, Rotation(rot_into_prev_branch));
            let key_rlc_mult_cur = meta.query_advice(key_rlc_mult, Rotation::cur());

            // Any rotation into branch children can be used:
            let key_rlc_branch = meta.query_advice(key_rlc, Rotation(rot_into_branch_init+1));
            let key_rlc_mult_branch = meta.query_advice(key_rlc_mult, Rotation(rot_into_branch_init+1));

            constraints.push((
                "branch key RLC same over all branch children with index > 0",
                q_not_first.clone()
                    * is_branch_child_prev.clone()
                    * is_branch_child_cur.clone()
                    * (key_rlc_cur.clone() - key_rlc_prev.clone()),
            ));
            constraints.push((
                "branch key RLC MULT same over all branch children with index > 0",
                q_not_first.clone()
                    * is_branch_child_prev.clone()
                    * is_branch_child_cur.clone()
                    * (key_rlc_mult_cur.clone() - key_rlc_mult_prev.clone()),
            ));

            constraints.push((
                "extension node row S key RLC is the same as branch key RLC when NOT extension node",
                q_not_first.clone()
                    * (one.clone() - is_branch_init_prev.clone()) // to prevent Poisoned Constraint due to rotation for is_extension_node
                    * (one.clone() - is_branch_child_prev.clone()) // to prevent Poisoned Constraint
                    * is_extension_s_row.clone()
                    * (one.clone() - is_extension_node.clone())
                    * (key_rlc_cur.clone() - key_rlc_prev.clone()),
            ));
            constraints.push((
                "extension node row S key RLC mult is the same as branch key RLC when NOT extension node",
                q_not_first.clone()
                    * (one.clone() - is_branch_init_prev.clone()) // to prevent Poisoned Constraint due to rotation for is_extension_node
                    * (one.clone() - is_branch_child_prev.clone()) // to prevent Poisoned Constraint
                    * is_extension_s_row.clone()
                    * (one.clone() - is_extension_node.clone())
                    * (key_rlc_mult_cur.clone() - key_rlc_mult_prev.clone()),
            ));

            constraints.push((
                "extension node row C key RLC is the same as branch key RLC when NOT extension node",
                q_not_first.clone()
                    * (one.clone() - is_branch_init_prev.clone()) // to prevent Poisoned Constraint due to rotation for is_extension_node
                    * (one.clone() - is_branch_child_prev.clone()) // to prevent Poisoned Constraint
                    * is_extension_c_row.clone()
                    * (one.clone() - is_extension_node.clone())
                    * (key_rlc_cur.clone() - key_rlc_prev.clone()),
            ));
            constraints.push((
                "extension node row C key RLC mult is the same as branch key RLC when NOT extension node",
                q_not_first.clone()
                    * (one.clone() - is_branch_init_prev.clone()) // to prevent Poisoned Constraint due to rotation for is_extension_node
                    * (one.clone() - is_branch_child_prev.clone()) // to prevent Poisoned Constraint
                    * is_extension_c_row.clone()
                    * (one.clone() - is_extension_node.clone())
                    * (key_rlc_mult_cur.clone() - key_rlc_mult_prev.clone()),
            ));

            
            // First level in account proof:

            let s_advices1 = meta.query_advice(s_advices[1], Rotation::prev());

            // skip 1 because s_advices[0] is 0 and doesn't contain any key info
            let mut first_level_long_even_rlc = s_advices1.clone() + compute_rlc(
                meta,
                s_advices.iter().skip(1).map(|v| *v).collect_vec(),
                0,
                one.clone(),
                -1,
                r_table.clone(),
            );
            first_level_long_even_rlc = first_level_long_even_rlc + modified_node_cur.clone() * c16.clone();

            // 
            constraints.push((
                "account first level long even",
                    q_not_first.clone()
                    * (one.clone() - is_branch_init_prev.clone()) // to prevent Poisoned Constraint due to rotation for is_extension_node
                    * (one.clone() - is_branch_child_prev.clone()) // to prevent Poisoned Constraint
                    * (one.clone() - not_first_level.clone())
                    * is_extension_node.clone()
                    * is_extension_c_row.clone()
                    * is_key_even.clone()
                    * is_long.clone()
                    * (first_level_long_even_rlc.clone() - key_rlc_cur.clone())
            )); // TODO: prepare test

            // TODO: all cases for first level account proof

            // First storage level:

            constraints.push((
                "storage first level long even",
                not_first_level.clone()
                    * is_account_leaf_storage_codehash_prev.clone()
                    * is_extension_node.clone()
                    * is_extension_c_row.clone()
                    * is_key_even.clone()
                    * is_long.clone()
                    * (first_level_long_even_rlc - key_rlc_cur.clone())
            ));

            // Not first level:

            // TODO: check key_rlp_mult (using lookup table and key len)

            let mut long_even_rlc = key_rlc_prev_level.clone() +
                s_advices1 * key_rlc_mult_prev_level.clone();
            // skip 1 because s_advices[0] is 0 and doesn't contain any key info, and skip another 1
            // because s_advices[1] is not to be multiplied by any r_table element (as it's in compute_rlc).
            long_even_rlc = long_even_rlc.clone() + compute_rlc(
                meta,
                s_advices.iter().skip(2).map(|v| *v).collect_vec(),
                0,
                key_rlc_mult_prev_level.clone(),
                -1,
                r_table.clone(),
            );

            constraints.push((
                "long even sel1 extension",
                not_first_level.clone()
                    * (one.clone() - is_account_leaf_storage_codehash_prev.clone())
                    * is_extension_node.clone()
                    * is_extension_c_row.clone()
                    * is_key_even.clone()
                    * is_long.clone()
                    * sel1.clone()
                    * (key_rlc_cur.clone() - long_even_rlc.clone())
            ));
            // We check branch key RLC in extension C row too (otherwise +rotation would be needed
            // because we first have branch rows and then extension rows):
            constraints.push((
                "long even sel1 branch",
                not_first_level.clone()
                    * (one.clone() - is_account_leaf_storage_codehash_prev.clone())
                    * is_extension_node.clone()
                    * is_extension_c_row.clone()
                    * is_key_even.clone()
                    * is_long.clone()
                    * sel1.clone()
                    * (key_rlc_branch.clone() - key_rlc_cur.clone() -
                        c16.clone() * modified_node_cur.clone() * key_rlc_mult_branch.clone())
            ));
            // TODO: extension -> branch mult constraint - depends on key len -
            // branch_mult = extension_mult * r^key_len
            /*
            constraints.push((
                "long even sel2",
                not_first_level.clone()
                    * (one.clone() - is_account_leaf_storage_codehash_prev.clone())
                    * is_extension_node.clone()
                    * is_extension_c_row.clone()
                    * is_key_even.clone()
                    * is_long.clone()
                    * sel2.clone()
                    * (key_rlc_cur.clone() - long_even_rlc.clone() - modified_node_cur.clone() * key_rlc_mult_prev_level.clone())
            ));
            */


            constraints
        });

        config
    }

    pub fn construct(config: ExtensionNodeKeyConfig) -> Self {
        Self {
            config,
            _marker: PhantomData,
        }
    }
}

impl<F: FieldExt> Chip<F> for ExtensionNodeKeyChip<F> {
    type Config = ExtensionNodeKeyConfig;
    type Loaded = ();

    fn config(&self) -> &Self::Config {
        &self.config
    }

    fn loaded(&self) -> &Self::Loaded {
        &()
    }
}
