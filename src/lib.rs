///
/// A simple graph api.
///
/// Features:
///
/// 1. Demonstrates rusts "interior mutability" pattern where we can
/// have immutable outer objects but change select sub fields.
///
/// 2. Demonstrates use of the Rc<T> smart pointer, this is required in
/// data structures such as graphs where nodes may have multiple owners.
/// I.e. some nodes may be referred to by many other nodes.
///
/// 3. Demonstrates the use of the mockall crates which assists in writing
/// unit tests with expectations.
///
/// 4. Demonstrates rusts unit testing capabilities by testing the public API.
///
/// 5. No use of clone() (other than cloning Rc<T> which is low cost), makes use
/// of references where possible.
///
/// Overview.
/// ---------
///
/// A graph is a data structure which is comprised of nodes (sometimes called
/// verticies) and edges. Nodes are connected by edges. It's an interesting
/// data structure from a rust point of view because graphs require multiple
/// ownership.
///
/// There is no logging implemented, I would expect the library user to implement
/// their own logging when interacting with the graph.
///
/// Rc only works in single threaded scenarios, I've kept things simple by only
/// supporting a single thread. Though it would be relatively straight forward to
/// use Arc<T> to support multithreading.
///
/// The public API is via the Graph structure.
use std::{cell::RefCell, collections::HashMap, hash::Hash, rc::Rc};

/// A type that allows shared ownership of a mutable value. Change Rc --> Arc
/// to support multithreaded ownership.
type MutableRc<T> = Rc<RefCell<T>>;

/// Use mockall to all the test code to set expectations on the routines
/// that should be called in various scenarios.
#[allow(unused)]
#[mockall::automock]
trait GraphOps<K, V> {
    /// Called when a node loses it's last dependent child, this could be used
    /// as a signal to delete the parent node the notification is received for.
    fn last_child_gone(&self, parent: &MutableRc<GraphNode<K, V>>);
}

/// These structures will be wrapped in a MutableRc in order to share
/// them throughout the graph.
#[allow(unused)]
pub struct GraphNode<K, V> {
    /// All the other nodes that point to this one.
    children: HashMap<K, MutableRc<GraphNode<K, V>>>,
    /// The object that is stored in the node, wrapped in RefCell
    /// so that the data can be mutated.
    data: RefCell<V>,
    /// Immutable key.
    key: K,
}

#[allow(unused)]
impl<K, V> GraphNode<K, V>
where
    K: Eq + Hash + Clone,
{
    /// Create a new graph node that can be stored in a Graph<K, V>
    pub fn new(data: V, key: K) -> MutableRc<Self> {
        Rc::new(RefCell::new(Self {
            data: RefCell::new(data),
            key,
            children: HashMap::new(),
        }))
    }

    /// Replace the data stored in the graph and return what was previously
    /// there.
    fn update_data(&self, data: V) -> V {
        self.data.replace(data)
    }

    /// Return true if the node has dependencies, otherwise return false.
    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    /// Add a dependent node to this node.
    fn add_child(&mut self, child_node: &MutableRc<Self>) {
        if !self.children.contains_key(&child_node.borrow().key) {
            self.children
                .insert(child_node.borrow().key.clone(), Rc::clone(child_node));
        }
    }

    /// Remove a dependent child from this node.
    fn remove_child(&mut self, child_node: &MutableRc<Self>) -> Option<MutableRc<GraphNode<K, V>>> {
        self.children.remove(&child_node.as_ref().borrow().key)
    }
}

#[allow(unused)]
struct Graph<K, V, O>
where
    O: GraphOps<K, V>,
{
    /// A hash map of nodes.
    nodes: HashMap<K, MutableRc<GraphNode<K, V>>>,
    /// Set of callbacks that can be used to notify when graph
    /// state changes in different ways.
    ops: O,
}

#[allow(unused)]
impl<K, V, O> Graph<K, V, O>
where
    K: Eq + Hash + Clone,
    O: GraphOps<K, V>,
{
    /// Create a new graph.
    pub fn new(ops: O) -> Self {
        Self {
            nodes: HashMap::new(),
            ops,
        }
    }

    /// Add a new node to the graph.
    pub fn add_node(&mut self, key: K, node: &MutableRc<GraphNode<K, V>>) {
        self.nodes.entry(key).or_insert_with(|| Rc::clone(node));
    }

    /// Remove a node from the graph. This will only succeed if the node has
    /// no dependencies left, if a node was removed then the routine returns
    /// true.
    pub fn remove_node(&mut self, key: &K) -> bool {
        if self
            .nodes
            .get(key)
            .map(|n| n.as_ref().borrow().children.is_empty())
            .unwrap_or(false)
        {
            self.nodes.remove(key);
            true
        } else {
            false
        }
    }

    /// Add a dependent child to this node.
    pub fn add_child(
        &self,
        parent: &MutableRc<GraphNode<K, V>>,
        child: &MutableRc<GraphNode<K, V>>,
    ) {
        parent.as_ref().borrow_mut().add_child(child);
    }

    /// Remove a dependent child from this node.
    ///
    /// The following trait callbacks may be called here:
    ///
    /// last_child_gone().
    pub fn remove_child(
        &self,
        parent: &MutableRc<GraphNode<K, V>>,
        child: &MutableRc<GraphNode<K, V>>,
    ) {
        if parent.as_ref().borrow_mut().remove_child(child).is_some()
            && !parent.as_ref().borrow().has_children()
        {
            self.ops.last_child_gone(parent);
        }
    }

    /// Get a node from the graph.
    pub fn get_node(&self, key: &K) -> Option<&MutableRc<GraphNode<K, V>>> {
        self.nodes.get(key)
    }
}

/// The test module uses 2 dependent objects to demonstrate the capabilities
/// of the API. We have an network interface object and a policy object, think
/// of the policy being bound to an interface. For example, a policy might say
/// drop all traffic destined to 10.0.0.0/24 on eth0. There is a dependency ordering
/// here in that the policy cannot be installed until the interface exists, thus
/// a graph is the perfect data structure for managing updates that may arrive
/// out of order.
#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;
    use ip_network::IpNetwork;
    use mockall::Sequence;

    /// Key types the graph supports.
    #[derive(Eq, Hash, PartialEq, Clone, Debug)]
    enum GraphKey {
        PolicyKey(u32),
        InterfaceKey(u32),
    }

    /// We can store these objects in the graph.
    struct Interface<'a> {
        /// The ID of the interface.
        #[allow(unused)]
        id: u32,
        /// The name of the interface, for example "eth0".
        #[allow(unused)]
        name: &'a str,
    }

    #[non_exhaustive]
    enum PolicyAction {
        Drop,
        Log,
    }

    /// We can also store these objects.
    struct DestNetworkPolicy<'a> {
        /// The ID of the policy.
        #[allow(unused)]
        id: u32,
        /// The description of the policy.
        #[allow(unused)]
        description: &'a str,
        /// The list of interfaces the policy should be applied on. In the real
        /// world we'd also need to know the direction the policy should be applied
        /// in, i.e. ingress or egress.
        #[allow(unused)]
        bound_to_interfaces: Vec<u32>,
        /// Match traffic to this destination network.
        #[allow(unused)]
        dst_addr: IpNetwork,
        /// Perform this action on the packet.
        #[allow(unused)]
        action: PolicyAction,
    }

    /// The type of objects that can be stored in the graph.
    enum GraphObject<'a> {
        #[allow(unused)]
        InterfaceObject(Interface<'a>),
        #[allow(unused)]
        DestNetworkPolicyObject(DestNetworkPolicy<'a>),
    }

    #[test]
    fn single_threaded() {
        let mut mock_ops = MockGraphOps::new();

        // Set expectations, i.e. routines we expect to be called in the GraphOps
        // trait.
        let mut seq = Sequence::new();
        mock_ops
            .expect_last_child_gone()
            .times(1)
            .returning(|_| ())
            .in_sequence(&mut seq);
        let mut g: Graph<GraphKey, GraphObject, MockGraphOps<GraphKey, GraphObject>> =
            Graph::new(mock_ops);

        let mut intf_node = GraphNode::new(
            GraphObject::InterfaceObject(Interface { id: 1, name: "eth" }),
            GraphKey::InterfaceKey(1),
        );

        g.add_node(GraphKey::InterfaceKey(1), &intf_node);

        let policy_node = GraphNode::new(
            GraphObject::DestNetworkPolicyObject(DestNetworkPolicy {
                id: 2,
                description: "Drop traffic to 10.0.0.0/24",
                bound_to_interfaces: vec![1],
                dst_addr: IpNetwork::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)), 24)
                    .expect("failed to build network object"),
                action: PolicyAction::Drop,
            }),
            GraphKey::PolicyKey(1),
        );

        g.add_node(GraphKey::PolicyKey(1), &policy_node);

        assert!(g.nodes.contains_key(&GraphKey::PolicyKey(1)));
        assert!(g.nodes.contains_key(&GraphKey::InterfaceKey(1)));

        // Add the interface as a parent of the policy, i.e. we cannot delete
        // the interface whilst the policy is still bound.
        g.add_child(&mut intf_node, &policy_node);

        assert!(intf_node
            .as_ref()
            .borrow()
            .children
            .contains_key(&GraphKey::PolicyKey(1)));

        // Remove the policy from the interface linkage, the last_child_gone
        // route should be called.
        g.remove_child(&mut intf_node, &policy_node);

        assert!(!intf_node
            .borrow()
            .children
            .contains_key(&GraphKey::PolicyKey(1)));

        // Remove the interface node.
        g.remove_node(&GraphKey::InterfaceKey(1));

        // Replace the policy node data, keep everything the same except the action.
        policy_node
            .as_ref()
            .borrow_mut()
            .update_data(GraphObject::DestNetworkPolicyObject(DestNetworkPolicy {
                id: 2,
                description: "Drop traffic to 10.0.0.0/24",
                bound_to_interfaces: vec![1],
                dst_addr: IpNetwork::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)), 24)
                    .expect("failed to build network object"),
                action: PolicyAction::Log,
            }));
    }
}
