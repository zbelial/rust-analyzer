use std::hash::{Hash, Hasher};
use std::collections::VecDeque;
use std::sync::{Arc, Weak};

use parking_lot::{RwLock, Mutex, RwLockUpgradableReadGuard};
use ra_syntax::{TreeArc, SourceFile};

#[derive(Debug)]
pub struct SourceManager {
    // stores `FileData`s with parsed trees
    cache: Mutex<BoundedQueue<Weak<FileData>>>,
}

impl std::panic::RefUnwindSafe for SourceManager {}

#[derive(Debug)]
pub struct FileData {
    text: Arc<String>,
    // Invariant: if the tree is `Some`, `FileData` belongs to `SourceManager`
    tree: RwLock<Option<TreeArc<SourceFile>>>,
}

impl std::panic::RefUnwindSafe for FileData {}

impl PartialEq for FileData {
    fn eq(&self, other: &FileData) -> bool {
        self.text == other.text
    }
}

impl Eq for FileData {}

impl Hash for FileData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text.hash(state)
    }
}

impl FileData {
    pub fn new(text: Arc<String>) -> FileData {
        FileData { text, tree: RwLock::new(None) }
    }
    pub fn text(&self) -> &Arc<String> {
        &self.text
    }
}

impl Default for SourceManager {
    fn default() -> SourceManager {
        SourceManager::with_capacity(64)
    }
}

impl SourceManager {
    pub fn with_capacity(cap: usize) -> SourceManager {
        SourceManager { cache: Mutex::new(BoundedQueue::with_capacity(cap)) }
    }
}

impl SourceManager {
    pub fn parse(&self, data: &Arc<FileData>) -> TreeArc<SourceFile> {
        let guard = data.tree.upgradable_read();
        match &*guard {
            Some(tree) => return tree.clone(),
            None => (),
        }
        let mut guard = RwLockUpgradableReadGuard::upgrade(guard);
        let tree = SourceFile::parse(&data.text);
        let mut cache = self.cache.lock();
        if let Some(evicted) = cache.push(Arc::downgrade(data)) {
            //TODO: figure out if this can deadlock
            if let Some(evicted) = evicted.upgrade() {
                *evicted.tree.write() = None;
            }
        }
        *guard = Some(tree.clone());
        tree
    }
}

#[derive(Debug)]
struct BoundedQueue<T> {
    cap: usize,
    items: VecDeque<T>,
}

impl<T> BoundedQueue<T> {
    pub fn with_capacity(cap: usize) -> BoundedQueue<T> {
        BoundedQueue { cap, items: VecDeque::with_capacity(cap) }
    }
    pub fn push(&mut self, item: T) -> Option<T> {
        let mut res = None;
        if self.items.len() == self.cap {
            res = self.items.pop_front();
        }
        self.items.push_back(item);
        res
    }
}
