//! Merkle implementation

/// Merkle tree for content verification
pub struct MerkleTree {
    /// Leaf hashes (chunk hashes)
    leaves: Vec<[u8; 32]>,
    /// Internal nodes (computed)
    nodes: Vec<[u8; 32]>,
}

impl MerkleTree {
    /// Create a new Merkle tree from chunk hashes
    pub fn from_leaves(leaves: Vec<[u8; 32]>) -> Self {
        let nodes = Self::build_tree(&leaves);
        Self { leaves, nodes }
    }

    /// Get the root hash
    pub fn root(&self) -> [u8; 32] {
        self.nodes.last().copied().unwrap_or([0u8; 32])
    }

    /// Generate a proof for a specific leaf
    ///
    /// Returns a vector of sibling hashes along the path from leaf to root.
    /// These are the hashes needed to reconstruct the root given the leaf.
    pub fn proof(&self, leaf_index: usize) -> Vec<[u8; 32]> {
        if leaf_index >= self.leaves.len() {
            return Vec::new();
        }

        let mut proof = Vec::new();
        let mut index = leaf_index;
        let mut level_start = 0;
        let mut level_size = self.leaves.len();

        // Walk up the tree, collecting sibling hashes
        while level_size > 1 {
            // Find the sibling index
            let sibling_index = if index % 2 == 0 {
                // Left child, sibling is right
                index + 1
            } else {
                // Right child, sibling is left
                index - 1
            };

            // Get the sibling hash
            let sibling_node_index = level_start + sibling_index;
            if sibling_index < level_size {
                proof.push(self.nodes[sibling_node_index]);
            } else {
                // Odd number of nodes - duplicate the last one
                proof.push(self.nodes[level_start + index]);
            }

            // Move to parent level
            index /= 2;
            level_start += level_size;
            level_size = (level_size + 1) / 2;
        }

        proof
    }

    /// Verify a proof
    ///
    /// Given a leaf hash, a proof (sibling path), the expected root,
    /// and the leaf index, verify that the leaf belongs to the tree.
    pub fn verify_proof(
        leaf: &[u8; 32],
        proof: &[[u8; 32]],
        root: &[u8; 32],
        leaf_index: usize,
    ) -> bool {
        let mut current_hash = *leaf;
        let mut current_index = leaf_index;

        // Walk up the tree, combining with siblings
        for sibling in proof {
            current_hash = if current_index % 2 == 0 {
                // Current is left, sibling is right
                hash_pair(&current_hash, sibling)
            } else {
                // Current is right, sibling is left
                hash_pair(sibling, &current_hash)
            };
            current_index /= 2;
        }

        // Check if we reached the expected root
        current_hash == *root
    }

    fn build_tree(leaves: &[[u8; 32]]) -> Vec<[u8; 32]> {
        if leaves.is_empty() {
            return vec![[0u8; 32]];
        }

        let mut nodes = leaves.to_vec();
        let mut level_start = 0;
        let mut level_size = leaves.len();

        while level_size > 1 {
            let next_level_size = (level_size + 1) / 2;

            for i in 0..next_level_size {
                let left_idx = level_start + i * 2;
                let right_idx = left_idx + 1;

                let left = nodes[left_idx];
                let right = if right_idx < level_start + level_size {
                    nodes[right_idx]
                } else {
                    left // Duplicate last node if odd
                };

                nodes.push(hash_pair(&left, &right));
            }

            level_start += level_size;
            level_size = next_level_size;
        }

        nodes
    }
}

fn hash_pair(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut combined = [0u8; 64];
    combined[..32].copy_from_slice(left);
    combined[32..].copy_from_slice(right);
    warp_hash::hash(&combined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_leaf() {
        let leaf = [1u8; 32];
        let tree = MerkleTree::from_leaves(vec![leaf]);
        assert_eq!(tree.root(), leaf);
    }

    #[test]
    fn test_two_leaves() {
        let leaf1 = [1u8; 32];
        let leaf2 = [2u8; 32];
        let tree = MerkleTree::from_leaves(vec![leaf1, leaf2]);

        // Root should be hash of both leaves
        let expected_root = hash_pair(&leaf1, &leaf2);
        assert_eq!(tree.root(), expected_root);
    }

    #[test]
    fn test_proof_single_leaf() {
        let leaf = [1u8; 32];
        let tree = MerkleTree::from_leaves(vec![leaf]);
        let proof = tree.proof(0);

        // Single leaf has no siblings in proof
        assert_eq!(proof.len(), 0);
        assert!(MerkleTree::verify_proof(&leaf, &proof, &tree.root(), 0));
    }

    #[test]
    fn test_proof_two_leaves() {
        let leaf1 = [1u8; 32];
        let leaf2 = [2u8; 32];
        let tree = MerkleTree::from_leaves(vec![leaf1, leaf2]);

        // Proof for leaf 0 should contain leaf 1 as sibling
        let proof0 = tree.proof(0);
        assert_eq!(proof0.len(), 1);
        assert_eq!(proof0[0], leaf2);
        assert!(MerkleTree::verify_proof(&leaf1, &proof0, &tree.root(), 0));

        // Proof for leaf 1 should contain leaf 0 as sibling
        let proof1 = tree.proof(1);
        assert_eq!(proof1.len(), 1);
        assert_eq!(proof1[0], leaf1);
        assert!(MerkleTree::verify_proof(&leaf2, &proof1, &tree.root(), 1));
    }

    #[test]
    fn test_proof_four_leaves() {
        let leaves: Vec<[u8; 32]> = (0..4)
            .map(|i| {
                let mut arr = [0u8; 32];
                arr[0] = i;
                arr
            })
            .collect();

        let tree = MerkleTree::from_leaves(leaves.clone());

        // Verify proof for each leaf
        for (i, leaf) in leaves.iter().enumerate() {
            let proof = tree.proof(i);
            // 4 leaves = depth 2, so proof should have 2 siblings
            assert_eq!(proof.len(), 2);
            assert!(MerkleTree::verify_proof(leaf, &proof, &tree.root(), i));
        }
    }

    #[test]
    fn test_proof_odd_leaves() {
        let leaves: Vec<[u8; 32]> = (0..5)
            .map(|i| {
                let mut arr = [0u8; 32];
                arr[0] = i;
                arr
            })
            .collect();

        let tree = MerkleTree::from_leaves(leaves.clone());

        // Verify proof for each leaf
        for (i, leaf) in leaves.iter().enumerate() {
            let proof = tree.proof(i);
            assert!(MerkleTree::verify_proof(leaf, &proof, &tree.root(), i));
        }
    }

    #[test]
    fn test_invalid_proof() {
        let leaf1 = [1u8; 32];
        let leaf2 = [2u8; 32];
        let tree = MerkleTree::from_leaves(vec![leaf1, leaf2]);

        let proof = tree.proof(0);

        // Wrong leaf should fail
        let wrong_leaf = [3u8; 32];
        assert!(!MerkleTree::verify_proof(&wrong_leaf, &proof, &tree.root(), 0));

        // Wrong root should fail
        let wrong_root = [0u8; 32];
        assert!(!MerkleTree::verify_proof(&leaf1, &proof, &wrong_root, 0));

        // Wrong index should fail
        assert!(!MerkleTree::verify_proof(&leaf1, &proof, &tree.root(), 1));
    }

    #[test]
    fn test_empty_tree() {
        let tree = MerkleTree::from_leaves(vec![]);
        assert_eq!(tree.root(), [0u8; 32]);
    }

    #[test]
    fn test_proof_out_of_bounds() {
        let leaf = [1u8; 32];
        let tree = MerkleTree::from_leaves(vec![leaf]);
        let proof = tree.proof(10);
        assert_eq!(proof.len(), 0);
    }

    #[test]
    fn test_hash_pair_deterministic() {
        let left = [1u8; 32];
        let right = [2u8; 32];

        let hash1 = hash_pair(&left, &right);
        let hash2 = hash_pair(&left, &right);

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_pair_order_matters() {
        let left = [1u8; 32];
        let right = [2u8; 32];

        let hash_lr = hash_pair(&left, &right);
        let hash_rl = hash_pair(&right, &left);

        assert_ne!(hash_lr, hash_rl);
    }
}
