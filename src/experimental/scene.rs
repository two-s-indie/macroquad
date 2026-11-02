use crate::camera::Camera2D;
pub use macroquad_macro::CapabilityTrait;
use std::{
    any::Any,
    cell::{Cell, RefCell},
    marker::PhantomData,
    ops::{Deref, DerefMut},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    thread::ThreadId,
};

#[rustfmt::skip]
pub trait Node {
    fn ready(_node: RefMut<Self>) where Self: Sized {}
    fn update(_node: RefMut<Self>) where Self: Sized {}
    fn fixed_update(_node: RefMut<Self>) where Self: Sized {}
    fn draw(_node: RefMut<Self>) where Self: Sized {}
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Id {
    scene: u64,
    id: usize,
    generation: u64,
}

/// A copyable identifier, not ownership of a node. Sending a handle does not
/// permit accessing its node from another thread or engine session.
pub struct Handle<T: 'static> {
    id: Option<Id>,
    // Identifies T without owning it: handles are Send even for non-Send nodes.
    _marker: PhantomData<fn() -> T>,
}

#[derive(Clone, Copy, Debug)]
pub struct HandleUntyped(Id);

impl<T> std::fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.id.fmt(f)
    }
}
impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Handle<T> {}

impl<T> Handle<T> {
    pub const fn null() -> Self {
        Self {
            id: None,
            _marker: PhantomData,
        }
    }
    pub fn untyped(&self) -> HandleUntyped {
        HandleUntyped(self.id.unwrap())
    }
    pub const fn as_trait<T1: ?Sized>(&self) {}

    pub(crate) fn lens<U, F>(self, project: F) -> Lens<T, U, F>
    where
        F: for<'a> FnMut(&'a mut T) -> &'a mut U,
    {
        assert!(self.id.is_some());
        Lens {
            handle: self,
            project,
            _marker: PhantomData,
        }
    }
}

/// Projects initialized data under a parent-node guard. A field reference
/// cannot escape the scoped operation or outlive that guard.
pub(crate) struct Lens<T: 'static, U, F> {
    handle: Handle<T>,
    project: F,
    _marker: PhantomData<fn() -> U>,
}
impl<T, U, F> Lens<T, U, F>
where
    F: for<'a> FnMut(&'a mut T) -> &'a mut U,
{
    pub fn with<R>(&mut self, use_field: impl FnOnce(&mut U) -> R) -> Option<R> {
        let mut node = unsafe { get_scene() }.get(self.handle)?;
        Some(use_field((self.project)(&mut node)))
    }
}

pub struct NodeWith<T> {
    pub node: HandleUntyped,
    pub capability: T,
}

/// Owns an exclusive borrow. Node data and capabilities remain valid when the
/// registry grows or the originating scene is destroyed.
pub struct RefMut<T: 'static> {
    inner: RefMutAny<'static>,
    handle: Handle<T>,
}
impl<T> RefMut<T> {
    pub const fn handle(&self) -> Handle<T> {
        self.handle
    }
    pub fn persist(&self) {
        self.inner.entry.permanent.set(true);
    }
    pub fn provides<S: Any + Copy>(&mut self, value: S) {
        self.inner
            .data
            .as_mut()
            .unwrap()
            .capabilities
            .push(Box::new(value));
    }
    pub fn delete(self) {
        self.inner.delete();
    }
}
impl<T> Deref for RefMut<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.inner.downcast_ref().expect("checked node type")
    }
}
impl<T> DerefMut for RefMut<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.inner.downcast_mut().expect("checked node type")
    }
}

/// Owns both the value and its stable entry. The lifetime parameter is retained
/// for compatibility; guards do not borrow the scene registry.
pub struct RefMutAny<'a> {
    data: Option<NodeData>,
    entry: Rc<NodeEntry>,
    handle: HandleUntyped,
    _marker: PhantomData<&'a ()>,
}
impl RefMutAny<'_> {
    /// Construct an identifier. Lookup validates its requested type.
    pub const fn handle<T>(&self) -> Handle<T> {
        Handle {
            id: Some(self.handle.0),
            _marker: PhantomData,
        }
    }
    pub fn delete(self) {
        let id = self.entry.id;
        // Restore the value before removing membership. No user destructor runs
        // here while the registry still owns this entry.
        drop(self);
        let removed = unsafe { get_scene() }.delete(id);
        // Destructors can look up, add, or delete other nodes.
        drop(removed);
    }
    /// Panics on a type mismatch, releasing this borrow during unwinding.
    pub fn to_typed<T: 'static>(mut self) -> RefMut<T> {
        assert!(self.is::<T>(), "node type mismatch");
        let handle = self.handle();
        let inner = RefMutAny {
            data: self.data.take(),
            entry: Rc::clone(&self.entry),
            handle: self.handle,
            _marker: PhantomData,
        };
        RefMut { inner, handle }
    }
}
impl Deref for RefMutAny<'_> {
    type Target = dyn Any;
    fn deref(&self) -> &Self::Target {
        &*self.data.as_ref().unwrap().value
    }
}
impl DerefMut for RefMutAny<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.data.as_mut().unwrap().value
    }
}
impl Drop for RefMutAny<'_> {
    fn drop(&mut self) {
        if let Some(data) = self.data.take() {
            // Only this guard can fill the empty slot. Move the previous value
            // out so an invariant failure never drops user data in a borrow.
            let previous = self.entry.data.replace(Some(data));
            assert!(previous.is_none(), "node already has a value");
        }
        // If detached, dropping our final Rc destroys the restored value.
    }
}

struct NodeData {
    value: Box<dyn Any>,
    capabilities: Vec<Box<dyn Any>>,
}
#[derive(Clone, Copy)]
struct Callbacks {
    ready: fn(RefMutAny<'static>),
    update: fn(RefMutAny<'static>),
    fixed_update: fn(RefMutAny<'static>),
    draw: fn(RefMutAny<'static>),
}
struct NodeEntry {
    id: Id,
    data: RefCell<Option<NodeData>>,
    permanent: Cell<bool>,
    initialized: Cell<bool>,
    callbacks: Callbacks,
    data_len: usize,
}
impl NodeEntry {
    fn new<T: Node + 'static>(id: Id, value: T) -> Self {
        Self {
            id,
            data: RefCell::new(Some(NodeData {
                value: Box::new(value),
                capabilities: vec![],
            })),
            permanent: Cell::new(false),
            initialized: Cell::new(false),
            callbacks: Callbacks {
                ready: |node| T::ready(node.to_typed()),
                update: |node| T::update(node.to_typed()),
                fixed_update: |node| T::fixed_update(node.to_typed()),
                draw: |node| T::draw(node.to_typed()),
            },
            data_len: size_of::<T>(),
        }
    }
    fn borrow(self: &Rc<Self>) -> Option<RefMutAny<'static>> {
        let data = self.data.borrow_mut().take()?;
        Some(RefMutAny {
            data: Some(data),
            entry: Rc::clone(self),
            handle: HandleUntyped(self.id),
            _marker: PhantomData,
        })
    }
    fn is_borrowed(&self) -> bool {
        self.data.borrow().is_none()
    }
}

static NEXT_SCENE_ID: AtomicU64 = AtomicU64::new(1);

fn next_scene_id() -> u64 {
    let mut id = NEXT_SCENE_ID.load(Ordering::Relaxed);
    loop {
        let next = id.checked_add(1).expect("scene identity exhausted");
        match NEXT_SCENE_ID.compare_exchange_weak(id, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return id,
            Err(current) => id = current,
        }
    }
}

struct Scene {
    id: u64,
    owner: ThreadId,
    dense: Vec<Id>,
    nodes: Vec<Option<Rc<NodeEntry>>>,
    free_slots: Vec<Id>,
    camera: [Option<Rc<Camera2D>>; 4],
    camera_pos: crate::Vec2,
    acc: f64,
    current_time: f64,
    in_fixed_update: bool,
}
impl Scene {
    fn new() -> Self {
        Self::new_at(crate::time::get_time())
    }
    fn new_at(now: f64) -> Self {
        let id = next_scene_id();
        Self {
            id,
            owner: std::thread::current().id(),
            dense: vec![],
            nodes: vec![],
            free_slots: vec![],
            camera: [Some(Rc::new(Camera2D::default())), None, None, None],
            camera_pos: crate::vec2(0., 0.),
            acc: 0.0,
            current_time: now,
            in_fixed_update: false,
        }
    }
    fn assert_owner(&self) {
        assert_eq!(
            self.owner,
            std::thread::current().id(),
            "scene accessed from another thread"
        );
    }
    fn check_clear(&self) {
        self.assert_owner();
        assert!(
            self.nodes
                .iter()
                .flatten()
                .all(|entry| entry.permanent.get() || !entry.is_borrowed()),
            "cannot clear a borrowed node"
        );
    }
    // Return detached entries: callers drop them AFTER releasing the registry.
    fn clear(&mut self) -> Vec<Rc<NodeEntry>> {
        self.check_clear();
        let mut removed = Vec::new();
        for slot in &mut self.nodes {
            if slot.as_ref().is_some_and(|entry| !entry.permanent.get()) {
                let entry = slot.take().unwrap();
                self.free_slots.push(entry.id);
                removed.push(entry);
            }
        }
        self.dense.retain(|id| self.nodes[id.id].is_some());
        removed
    }
    fn entry(&self, id: Id) -> Option<&Rc<NodeEntry>> {
        if self.owner != std::thread::current().id() || id.scene != self.id {
            return None;
        }
        let entry = self.nodes.get(id.id)?.as_ref()?;
        (entry.id == id).then_some(entry)
    }
    fn get_any(&self, handle: HandleUntyped) -> Option<RefMutAny<'static>> {
        self.entry(handle.0)?.borrow()
    }
    fn get<T>(&self, handle: Handle<T>) -> Option<RefMut<T>> {
        let node = self.get_any(HandleUntyped(handle.id?))?;
        if !node.is::<T>() {
            return None;
        }
        Some(node.to_typed())
    }
    fn iter(&self) -> MagicVecIterator {
        self.assert_owner();
        MagicVecIterator {
            ids: self.dense.clone().into_iter(),
        }
    }
    fn add_node<T: Node + 'static>(&mut self, data: T) -> Handle<T> {
        self.assert_owner();
        // Exhausted generations retire slots instead of reviving old handles.
        let id = loop {
            match self.free_slots.pop() {
                Some(old) => {
                    if let Some(generation) = old.generation.checked_add(1) {
                        break Id { generation, ..old };
                    }
                }
                None => {
                    let id = Id {
                        scene: self.id,
                        id: self.nodes.len(),
                        generation: 0,
                    };
                    self.nodes.push(None);
                    break id;
                }
            }
        };
        self.nodes[id.id] = Some(Rc::new(NodeEntry::new(id, data)));
        self.dense.push(id);
        Handle {
            id: Some(id),
            _marker: PhantomData,
        }
    }
    fn delete(&mut self, id: Id) -> Option<Rc<NodeEntry>> {
        self.assert_owner();
        let entry = self.entry(id)?;
        assert!(!entry.is_borrowed(), "cannot delete a borrowed node");
        let removed = self.nodes[id.id].take();
        self.dense.retain(|current| *current != id);
        self.free_slots.push(id);
        removed
    }
    fn advance_time(&mut self, new_time: f64) {
        let mut frame_time = new_time - self.current_time;
        // Preserve legacy refresh-interval snapping and the strict fixed boundary.
        if (frame_time - 1.0 / 120.0).abs() < 0.0002 {
            frame_time = 1.0 / 120.0;
        } else if (frame_time - 1.0 / 60.0).abs() < 0.0002 {
            frame_time = 1.0 / 60.0;
        } else if (frame_time - 1.0 / 30.0).abs() < 0.0002 {
            frame_time = 1.0 / 30.0;
        }
        self.current_time = new_time;
        self.acc += frame_time;
    }
}

/// Snapshots identities. Additions are excluded; deleted/reused slots and nodes
/// currently borrowed by another guard are skipped.
pub struct MagicVecIterator {
    ids: std::vec::IntoIter<Id>,
}
impl Iterator for MagicVecIterator {
    type Item = RefMutAny<'static>;
    fn next(&mut self) -> Option<Self::Item> {
        for id in self.ids.by_ref() {
            if let Some(node) = get_untyped_node(HandleUntyped(id)) {
                return Some(node);
            }
        }
        None
    }
}

static mut SCENE: Option<Scene> = None;

// Step 3 replaces this legacy global with scoped thread-local storage.
// Owned guards never contain references into this registry.
unsafe fn get_scene() -> &'static mut Scene {
    SCENE.get_or_insert_with(Scene::new)
}

pub(crate) fn allocated_memory() -> usize {
    unsafe { get_scene() }
        .nodes
        .iter()
        .flatten()
        .map(|node| node.data_len)
        .sum()
}
pub fn clear() {
    unsafe { get_scene() }.check_clear();
    crate::experimental::coroutines::stop_all_coroutines();
    clear_nodes();
}
fn clear_nodes() {
    let removed = unsafe { get_scene() }.clear();
    drop(removed);
}

/// Get a node; panic if its handle is foreign, stale, wrongly typed, or borrowed.
pub fn get_node<T: Node>(handle: Handle<T>) -> RefMut<T> {
    try_get_node(handle).unwrap_or_else(|| panic!("No such node: {:?}", handle.id))
}
pub fn try_get_node<T: Node>(handle: Handle<T>) -> Option<RefMut<T>> {
    unsafe { get_scene() }.get(handle)
}
pub fn get_untyped_node(handle: HandleUntyped) -> Option<RefMutAny<'static>> {
    unsafe { get_scene() }.get_any(handle)
}
pub fn camera_pos() -> crate::Vec2 {
    unsafe { get_scene() }.camera_pos
}
pub fn set_camera(n: usize, camera: Option<Camera2D>) {
    assert!(n <= 4);
    let previous = std::mem::replace(&mut unsafe { get_scene() }.camera[n], camera.map(Rc::new));
    drop(previous);
}
pub fn add_node<T: Node>(node: T) -> Handle<T> {
    unsafe { get_scene() }.add_node(node)
}
pub(crate) fn update() {
    update_with(
        crate::time::get_time,
        |camera| {
            crate::prelude::push_camera_state();
            crate::prelude::set_camera(camera);
        },
        crate::prelude::pop_camera_state,
    );
}

// Dispatch without a registry borrow, including destructors triggered by a
// callback. Keep separate per-pass snapshots and legacy addition eligibility.
fn update_with(
    clock: impl FnOnce() -> f64,
    mut push_camera: impl FnMut(&Camera2D),
    mut pop_camera: impl FnMut(),
) {
    for node in all_nodes() {
        if !node.entry.initialized.replace(true) {
            let ready = node.entry.callbacks.ready;
            ready(node);
        }
    }
    let now = clock();
    unsafe { get_scene() }.advance_time(now);
    for node in all_nodes() {
        let update = node.entry.callbacks.update;
        update(node);
    }
    loop {
        let run_fixed = {
            let scene = unsafe { get_scene() };
            if scene.acc > CONST_FPS {
                scene.acc -= CONST_FPS;
                true
            } else {
                false
            }
        };
        if !run_fixed {
            break;
        }
        for node in all_nodes() {
            let fixed_update = node.entry.callbacks.fixed_update;
            unsafe { get_scene() }.in_fixed_update = true;
            fixed_update(node);
            unsafe { get_scene() }.in_fixed_update = false;
        }
    }
    for n in 0..4 {
        let camera = unsafe { get_scene() }.camera[n].clone();
        if let Some(camera) = camera {
            unsafe { get_scene() }.camera_pos = camera.target;
            push_camera(&camera);
            for node in all_nodes() {
                let draw = node.entry.callbacks.draw;
                draw(node);
            }
            pop_camera();
        }
    }
}
pub fn all_nodes() -> MagicVecIterator {
    unsafe { get_scene() }.iter()
}
pub fn find_node_by_type<T: Any>() -> Option<RefMut<T>> {
    find_nodes_by_type().next()
}
pub fn find_nodes_with<T: Any + Copy>() -> impl Iterator<Item = NodeWith<T>> {
    all_nodes().filter_map(|node| {
        node.data
            .as_ref()
            .unwrap()
            .capabilities
            .iter()
            .find_map(|capability| capability.downcast_ref::<T>())
            .map(|capability| NodeWith {
                node: HandleUntyped(node.entry.id),
                capability: *capability,
            })
    })
}
pub fn find_nodes_by_type<T: Any>() -> impl Iterator<Item = RefMut<T>> {
    all_nodes()
        .filter(|node| node.is::<T>())
        .map(|node| node.to_typed())
}
const CONST_FPS: f64 = 1.0 / 60.;
pub(crate) fn in_fixed_update() -> bool {
    unsafe { get_scene() }.in_fixed_update
}
pub(crate) const fn fixed_frame_time() -> f32 {
    CONST_FPS as _
}

#[cfg(test)]
mod tests;
