use crate::{
    block::Commitment::{self, ChainHistoryActivationReserved},
    serialization::ZcashDeserializeInto,
};

use crate::{history_tree::HistoryTreeError, primitives::zcash_history::*};
use color_eyre::eyre;
use eyre::Result;

/// Test the MMR tree using the activation block of a network upgrade
/// and its next block.
#[test]
fn tree() -> Result<()> {
    for network in Network::iter() {
        tree_for_network_upgrade(&network, NetworkUpgrade::Heartwood)?;
        tree_for_network_upgrade(&network, NetworkUpgrade::Canopy)?;
    }
    Ok(())
}

fn tree_for_network_upgrade(network: &Network, network_upgrade: NetworkUpgrade) -> Result<()> {
    let (blocks, sapling_roots) = network.block_sapling_roots_map();

    let height = network_upgrade.activation_height(network).unwrap().0;

    // Load Block 0 (activation block of the given network upgrade)
    let block0 = Arc::new(
        blocks
            .get(&height)
            .expect("test vector exists")
            .zcash_deserialize_into::<Block>()
            .expect("block is structurally valid"),
    );

    // Check its commitment
    let commitment0 = block0.commitment(network)?;
    if network_upgrade == NetworkUpgrade::Heartwood {
        // Heartwood is the only upgrade that has a reserved value.
        // (For other upgrades we could compare with the expected commitment,
        // but we haven't calculated them.)
        assert_eq!(commitment0, ChainHistoryActivationReserved);
    }

    // Build initial MMR tree with only Block 0
    let sapling_root0 =
        sapling::tree::Root::try_from(**sapling_roots.get(&height).expect("test vector exists"))?;
    let (mut tree, _) = Tree::<V1>::new_from_block(
        network,
        block0,
        BlockCommitmentTreeRoots {
            sapling: &sapling_root0,
            orchard: &Default::default(),
            ironwood: &Default::default(),
        },
    )?;

    // Compute root hash of the MMR tree, which will be included in the next block
    let hash0 = tree.hash();

    // Load Block 1 (activation + 1)
    let block1 = Arc::new(
        blocks
            .get(&(height + 1))
            .expect("test vector exists")
            .zcash_deserialize_into::<Block>()
            .expect("block is structurally valid"),
    );

    // Check its commitment
    let commitment1 = block1.commitment(network)?;
    assert_eq!(commitment1, Commitment::ChainHistoryRoot(hash0));

    // Append Block to MMR tree
    let sapling_root1 = sapling::tree::Root::try_from(
        **sapling_roots
            .get(&(height + 1))
            .expect("test vector exists"),
    )?;
    let append = tree
        .append_leaf(
            block1,
            BlockCommitmentTreeRoots {
                sapling: &sapling_root1,
                orchard: &Default::default(),
                ironwood: &Default::default(),
            },
        )
        .unwrap();

    // Tree how has 3 nodes: two leaves for each block, and one parent node
    // which is the new root
    assert_eq!(tree.inner.len(), 3);
    // Two nodes were appended: the new leaf and the parent node
    assert_eq!(append.len(), 2);

    Ok(())
}

/// A network upgrade without a consensus branch ID must make the MMR tree constructors return
/// [`HistoryTreeError::MissingBranchId`] instead of panicking.
///
/// In production builds this is what happens to a configured network that activates an upgrade
/// whose branch ID is only compiled into test builds (such as NU7), so the test uses upgrades that
/// have no branch ID in any build.
#[test]
fn tree_constructors_reject_missing_branch_id() -> Result<()> {
    let _init_guard = zebra_test::init();

    for network in Network::iter() {
        for network_upgrade in [NetworkUpgrade::Genesis, NetworkUpgrade::BeforeOverwinter] {
            assert_eq!(network_upgrade.branch_id(), None);

            let error = Tree::<V1>::new_from_cache(
                &network,
                network_upgrade,
                1,
                &BTreeMap::new(),
                &BTreeMap::new(),
            )
            .expect_err("an upgrade without a branch ID must not build a tree");
            assert_eq!(error, HistoryTreeError::MissingBranchId(network_upgrade));
        }
    }

    // The genesis block is in the `Genesis` network upgrade, which has no branch ID.
    let network = Network::Mainnet;
    let genesis_block = Arc::new(
        zebra_test::vectors::BLOCK_MAINNET_GENESIS_BYTES.zcash_deserialize_into::<Block>()?,
    );
    let roots = BlockCommitmentTreeRoots {
        sapling: &Default::default(),
        orchard: &Default::default(),
        ironwood: &Default::default(),
    };
    let expected = HistoryTreeError::MissingBranchId(NetworkUpgrade::Genesis);

    let error = Tree::<V1>::new_from_block(&network, genesis_block.clone(), roots)
        .expect_err("an upgrade without a branch ID must not build a V1 leaf");
    assert_eq!(error, expected);

    let error = Tree::<V2>::new_from_block(&network, genesis_block.clone(), roots)
        .expect_err("an upgrade without a branch ID must not build a V2 leaf");
    assert_eq!(error, expected);

    let error = Tree::<V3>::new_from_block(&network, genesis_block, roots)
        .expect_err("an upgrade without a branch ID must not build a V3 leaf");
    assert_eq!(error, expected);

    Ok(())
}
