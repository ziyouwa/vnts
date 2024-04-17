#![allow(dead_code)]
use std::cmp::Ordering;
use std::collections::HashMap;

use std::hash::Hash;
use std::ops::Add;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_utils::atomic::AtomicCell;
use parking_lot::RwLock;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct ExpireMap<K, V> {
    base: Arc<RwLock<HashMap<K, Value<V>>>>,
    sender: mpsc::Sender<DelayedTask<K>>,
}

struct Value<V> {
    val: V,
    deadline: AtomicCell<Instant>,
    expire: Duration,
}

impl<K, V> ExpireMap<K, V> {
    pub fn new<F>(call: F) -> ExpireMap<K, V>
    where
        F: Fn(K, V) + Send + 'static,
        K: Eq + Hash + Clone + Sync + Send + 'static,
        V: Clone + Sync + Send + 'static,
    {
        let (sender, mut receiver) = mpsc::channel::<DelayedTask<_>>(64);
        let base: Arc<RwLock<HashMap<K, Value<V>>>> = Arc::new(RwLock::new(HashMap::with_capacity(128)));
        let cloned_base = base.clone();
        tokio::spawn(async move {
            while let Ok(task) = receiver.try_recv() {
                // 任务已过期
                if task.time < Instant::now() {
                    let mut events = cloned_base.write();
                    if let Some(v) = events.get(&task.k) {
                        if v.deadline.load() <= Instant::now() {
                            call(task.k.clone(), v.val.clone());
                        }
                    }
                    events.remove(&task.k);
                }
            }
        });
        Self { base, sender }
    }
}

impl<K, V> ExpireMap<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    pub fn size(&self) -> usize {
        self.base.read().len()
    }
    pub async fn insert(&self, k: K, val: V, expire: Duration) {
        let instant = Instant::now().add(expire);
        {
            let value = Value {
                val,
                deadline: AtomicCell::new(instant),
                expire,
            };
            let mut write_guard = self.base.write();
            write_guard.insert(k.clone(), value);
        }
        //投入过期监听
        if let Err(e) = self.sender.send(DelayedTask { k, time: instant }).await {
            log::error!("发送失败:{:?}", e);
        }
    }
    pub fn get_and_renew(&self, k: &K) -> Option<V> {        
        if let Some(v) = self.base.read().get(k) {
            // 刷新过期时间
            v.deadline.store(Instant::now().add(v.expire));
            Some(v.val.clone())
        } else {
            None
        }
    }
    pub fn get_val(&self, k: &K) -> Option<V> {
        self.base.read().get(k).map(|v| v.val.clone())
    }

    pub async fn optionally_get_with<F>(&self, k: K, f: F) -> V
    where
        F: FnOnce() -> (Duration, V),
    {
        let (v, time) = {
            let mut write_guard = self.base.write();
            if let Some(v) = write_guard.get(&k) {
                // 延长过期时间
                v.deadline.store(Instant::now().add(v.expire));
                (v.val.clone(), None)
            } else {
                let (expire, val) = f();
                let deadline = Instant::now().add(expire);
                let value = Value {
                    val: val.clone(),
                    deadline: AtomicCell::new(deadline),
                    expire,
                };
                write_guard.insert(k.clone(), value);
                (val, Some(deadline))
            }
        };
        if let Some(time) = time {
            if let Err(e) = self.sender.send(DelayedTask { k, time }).await {
                log::error!("发送失败:{:?}", e);
            }
        }
        v
    }
    pub fn key_values(&self) -> Vec<(K, V)> {
        self.base
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), v.val.clone()))
            .collect()
    }
}

struct DelayedTask<K> {
    k: K,
    time: Instant,
}

impl<K> Eq for DelayedTask<K> {}

impl<K> PartialEq for DelayedTask<K> {
    fn eq(&self, other: &Self) -> bool {
        self.time.eq(&other.time)
    }
}

#[allow(clippy::non_canonical_partial_ord_impl)]
impl<K> PartialOrd for DelayedTask<K> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.time.partial_cmp(&other.time).map(|ord| ord.reverse())
    }
}

impl<K> Ord for DelayedTask<K> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.time.cmp(&other.time).reverse()
    }
}
