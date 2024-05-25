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

/*
rust编程中，针对泛型K、V，需要一个数据结构ExpireMap，满足如下条件：
1  ExpireMap能保存不定数量的(k, v)实例
2  ExpireMap实例中，对每个元素e, 在e的expose时间超时之后，调用e的f，f的参数为e里面的k, v
3  ExpireMap中插入元素时传入 k、v、expose 和 f
4  在某个元素的 expose 超时之后，需要确保这个元素的 f 被通知到并执行，如果多个元素同时到期则按顺序执行
请给出示例代码
 */
struct Value<V> {
    val: V,
    deadline: AtomicCell<Instant>,
    expire: Duration,
}

impl<K, V> ExpireMap<K, V> {
    pub fn new<F>(call: F) -> ExpireMap<K, V>
    where
        F: Fn(K, V) + Send + 'static,
        K: Eq + PartialEq + Hash + Clone + Sync + Send + 'static,
        V: Clone + Sync + Send + 'static,
    {
        let (sender, mut receiver) = mpsc::channel::<DelayedTask<_>>(64);
        let base: Arc<RwLock<HashMap<K, Value<V>>>> =
            Arc::new(RwLock::new(HashMap::with_capacity(128)));
        let cloned_base = base.clone();
        tokio::spawn(async move {
            while let Some(task) = receiver.recv().await {
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

    pub fn size(&self) -> usize {
        self.base.read().len()
    }

    pub async fn insert(&self, k: K, val: V, expire: Duration)
    where
        K: Eq + PartialEq + Hash + Clone + Sync + Send + 'static,
        V: Clone + Sync + Send + 'static,
    {
        let instant = Instant::now().add(expire);

        {
            let value = Value {
                val,
                deadline: AtomicCell::new(instant),
                expire,
            };
            let mut write_guard = self.base.write();
            write_guard.insert(k.clone(), value);
            drop(write_guard);
        }

        let k1 = k.clone();
        let s = self.sender.clone();
        tokio::spawn(async move {
            tokio::time::sleep(expire).await;
            //投入过期监听
            if let Err(e) = s
                .send(DelayedTask {
                    k: k1,
                    time: instant,
                })
                .await
            {
                log::error!("发送失败:{:?}", e);
            }
        });
    }

    pub fn get_and_renew(&self, k: &K) -> Option<V>
    where
        K: Eq + PartialEq + Hash + Clone + Sync + Send + 'static,
        V: Clone + Sync + Send + 'static,
    {
        if let Some(v) = self.base.read().get(k) {
            // 刷新过期时间
            v.deadline.store(Instant::now().add(v.expire));
            Some(v.val.clone())
        } else {
            None
        }
    }
    pub fn get_val(&self, k: &K) -> Option<V>
    where
        K: Eq + PartialEq + Hash + Clone + Sync + Send + 'static,
        V: Clone + Sync + Send + 'static,
    {
        self.base.read().get(k).map(|v| v.val.clone())
    }

    pub async fn optionally_get_with<F>(&self, k: K, f: F) -> V
    where
        F: FnOnce() -> (Duration, V),
        K: Eq + PartialEq + Hash + Clone + Sync + Send + 'static,
        V: Clone + Sync + Send + 'static,
    {
        let mut write_guard = self.base.write();
        if let Some(v) = write_guard.get(&k) {
            // 延长过期时间
            v.deadline.store(Instant::now().add(v.expire));
            v.val.clone()
        } else {
            let (expire, val) = f();
            let deadline = Instant::now().add(expire);
            let value = Value {
                val: val.clone(),
                deadline: AtomicCell::new(deadline),
                expire,
            };
            write_guard.insert(k.clone(), value);

            val
        }
    }
    pub fn key_values(&self) -> Vec<(K, V)>
    where
        K: Eq + PartialEq + Hash + Clone + Sync + Send + 'static,
        V: Clone + Sync + Send + 'static,
    {
        self.base
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), v.val.clone()))
            .collect()
    }
}

#[derive(Debug, Clone)]
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

#[cfg(test)]

mod test {
    use super::*;

    fn f(k: &str, v: &str) {
        println!("{k:?}-{:?}", v);
    }
    #[tokio::test]
    async fn test_expire_map() {
        let t1 = ExpireMap::new(f);
        t1.insert("a1", "v1", Duration::from_secs(2)).await;
        t1.insert("a2", "v1", Duration::from_secs(2)).await;
        t1.insert("a3", "v1", Duration::from_secs(2)).await;
        t1.insert("a4", "v1", Duration::from_secs(2)).await;
        t1.insert("a5", "v1", Duration::from_secs(2)).await;
        t1.insert("a6", "v1", Duration::from_secs(2)).await;
        t1.insert("a7", "v1", Duration::from_secs(2)).await;
        t1.insert("a8", "v1", Duration::from_secs(2)).await;
        t1.insert("a9", "v1", Duration::from_secs(2)).await;
        t1.insert("aa", "v1", Duration::from_secs(2)).await;
        t1.insert("ab", "v1", Duration::from_secs(2)).await;
        println!("a: {}", t1.base.read().get("a1").unwrap().val);
        tokio::time::sleep(Duration::from_secs(35)).await;
    }
}
