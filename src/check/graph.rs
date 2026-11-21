//! Dependency-ordered strongly connected components of a call graph.
//!
//! An edge points from a function to a function it calls. Components are
//! returned with dependencies first. Iterative traversals avoid using the host
//! stack for long chains of declarations.

pub(super) fn components(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = edges.len();
    let mut reverse = vec![Vec::new(); n];
    for (from, targets) in edges.iter().enumerate() {
        for &to in targets {
            reverse[to].push(from);
        }
    }

    let mut visited = vec![false; n];
    let mut finish = Vec::with_capacity(n);
    for start in 0..n {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut stack = vec![(start, 0)];
        while let Some((node, next)) = stack.last_mut() {
            if *next < edges[*node].len() {
                let child = edges[*node][*next];
                *next += 1;
                if !visited[child] {
                    visited[child] = true;
                    stack.push((child, 0));
                }
            } else {
                finish.push(*node);
                stack.pop();
            }
        }
    }

    visited.fill(false);
    let mut groups = Vec::new();
    for &start in finish.iter().rev() {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut group = Vec::new();
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            group.push(node);
            for &child in &reverse[node] {
                if !visited[child] {
                    visited[child] = true;
                    stack.push(child);
                }
            }
        }
        group.sort_unstable();
        groups.push(group);
    }
    groups.reverse();
    groups
}

#[cfg(test)]
mod tests {
    use super::components;

    #[test]
    fn recursive_components_follow_their_dependencies() {
        let edges = vec![vec![1], vec![2], vec![1, 3], vec![], vec![0, 3], vec![5]];
        let groups = components(&edges);
        let position = |node| {
            groups
                .iter()
                .position(|group| group.contains(&node))
                .unwrap()
        };
        assert_eq!(groups[position(1)], vec![1, 2]);
        assert!(position(3) < position(1));
        assert!(position(1) < position(0));
        assert!(position(0) < position(4));
        assert_eq!(groups[position(5)], vec![5]);
    }

    #[test]
    fn exhaustive_small_graphs_match_reachability_oracle() {
        // Every directed graph through four nodes, including self-edges and
        // disconnected nodes. The oracle is independent of DFS finish order.
        for n in 0..=4 {
            for mask in 0usize..(1usize << (n * n)) {
                let mut edges = vec![Vec::new(); n];
                let mut reachable = vec![vec![false; n]; n];
                for (from, (targets, row)) in edges.iter_mut().zip(&mut reachable).enumerate() {
                    row[from] = true;
                    for (to, entry) in row.iter_mut().enumerate() {
                        if mask & (1 << (from * n + to)) != 0 {
                            targets.push(to);
                            *entry = true;
                        }
                    }
                }
                for via in 0..n {
                    let via_row = reachable[via].clone();
                    for row in &mut reachable {
                        if row[via] {
                            for (entry, via_entry) in row.iter_mut().zip(&via_row) {
                                *entry |= *via_entry;
                            }
                        }
                    }
                }
                let groups = components(&edges);
                let mut positions = vec![None; n];
                for (position, group) in groups.iter().enumerate() {
                    assert!(!group.is_empty());
                    assert!(group.windows(2).all(|pair| pair[0] < pair[1]));
                    for &node in group {
                        assert!(positions[node].replace(position).is_none());
                    }
                }
                assert!(positions.iter().all(Option::is_some));
                for from in 0..n {
                    for to in 0..n {
                        let mutually_reachable = reachable[from][to] && reachable[to][from];
                        assert_eq!(
                            positions[from] == positions[to],
                            mutually_reachable,
                            "n={n}, mask={mask}, from={from}, to={to}"
                        );
                        if reachable[from][to] && !mutually_reachable {
                            assert!(
                                positions[to] < positions[from],
                                "dependency order: n={n}, mask={mask}, from={from}, to={to}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn duplicate_edges_do_not_duplicate_nodes() {
        assert_eq!(
            components(&[vec![1, 1], vec![0, 0, 2, 2], vec![2, 2]]),
            vec![vec![2], vec![0, 1]]
        );
    }

    #[test]
    fn long_chain_does_not_recurse() {
        let n = 20_000;
        let edges: Vec<_> = (0..n)
            .map(|i| if i + 1 < n { vec![i + 1] } else { vec![] })
            .collect();
        let groups = components(&edges);
        assert_eq!(groups.len(), n);
        assert_eq!(groups.first().unwrap(), &[n - 1]);
        assert_eq!(groups.last().unwrap(), &[0]);
    }
}
