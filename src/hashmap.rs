use std::hash::Hash;

pub type HashMap<K, V> = hashbrown::HashMap<K, V>;

pub trait GetOrInsert<K: Eq + Hash + Clone, V> {
        fn get_or_insert(&mut self, k: &K, v: V) -> &V;
        fn get_mut_or_insert(&mut self, k: &K, v: V) -> &mut V;
        fn get_or_insert_with(&mut self, k: &K, f: impl FnOnce() -> V) -> &V;
        fn get_mut_or_insert_with(&mut self, k: &K, f: impl FnOnce() -> V) -> &mut V;
}

impl<K: Eq + Hash + Clone, V> GetOrInsert<K, V> for HashMap<K, V> {
        fn get_or_insert(&mut self, k: &K, v: V) -> &V {
                self.raw_entry_mut().from_key(k).or_insert_with(|| (k.clone(), v)).1
        }

        fn get_mut_or_insert(&mut self, k: &K, v: V) -> &mut V {
                self.raw_entry_mut().from_key(k).or_insert_with(|| (k.clone(), v)).1
        }

        fn get_or_insert_with(&mut self, k: &K, f: impl FnOnce() -> V) -> &V {
                self.raw_entry_mut().from_key(k).or_insert_with(|| (k.clone(), f())).1
        }

        fn get_mut_or_insert_with(&mut self, k: &K, f: impl FnOnce() -> V) -> &mut V {
                self.raw_entry_mut().from_key(k).or_insert_with(|| (k.clone(), f())).1
        }
}

pub trait GetOrInsertDefault<K: Eq + Hash + Clone, V: Default> {
        fn get_or_insert_default(&mut self, k: &K) -> &V;
        fn get_mut_or_insert_default(&mut self, k: &K) -> &mut V;
}

impl<K: Eq + Hash + Clone, V: Default> GetOrInsertDefault<K, V> for HashMap<K, V> {
        fn get_or_insert_default(&mut self, k: &K) -> &V {
                self.get_or_insert_with(k, || Default::default())
        }

        fn get_mut_or_insert_default(&mut self, k: &K) -> &mut V {
                self.get_mut_or_insert_with(k, || Default::default())
        }
}
