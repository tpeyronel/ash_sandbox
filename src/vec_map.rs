use std::{
        ops::{Index, IndexMut},
};

use bitflags::_core::marker::PhantomData;

pub struct VecMap<K: VecMapKey, V> {
        values:   Vec<V>,
        _phantom: PhantomData<K>,
}

impl<K: VecMapKey, V> VecMap<K, V> {
        pub fn new() -> Self {
                Self {
                        values:   Vec::new(),
                        _phantom: Default::default(),
                }
        }

        pub fn insert(&mut self, value: V) -> K {
                self.values.push(value);
                K::from(self.values.len() - 1)
        }

        pub fn at(&self, key: K) -> &V {
                &self.values[key.get()]
        }

        pub fn at_mut(&mut self, key: K) -> &mut V {
                &mut self.values[key.get()]
        }
}

impl<K: VecMapKey, V> Index<K> for VecMap<K, V> {
        type Output = V;

        #[inline]
        fn index(&self, key: K) -> &Self::Output {
                self.at(key)
        }
}

impl<K: VecMapKey, V> IndexMut<K> for VecMap<K, V> {
        #[inline]
        fn index_mut(&mut self, key: K) -> &mut Self::Output {
                self.at_mut(key)
        }
}

impl<K: VecMapKey, V> Default for VecMap<K, V> {
        fn default() -> Self {
                Self::new()
        }
}

// pub struct VecMapIntoIterator {}
//
// impl<'a, K: VecMapKey, V> Iterator for &'a VecMap<K, V> {
//         type Item = (K, &'a V);
//
//         fn next(&mut self) -> Option<Self::Item> {
//         }
// }

fn vec_map_iter_fn<K: VecMapKey, V>((i, v): (usize, &V)) -> (K, &V) {
        (K::from(i), v)
}

type VecMapIterFn<K, V> = for<'a> fn((usize, &V)) -> (K, &V);

impl<'a, K: VecMapKey, V> IntoIterator for &'a VecMap<K, V> {
        type Item = (K, &'a V);
        type IntoIter = std::iter::Map<std::iter::Enumerate<std::slice::Iter<'a, V>>, VecMapIterFn<K, V>>;

        fn into_iter(self) -> Self::IntoIter {
                self.values
                        .iter()
                        .enumerate()
                        .map(vec_map_iter_fn as VecMapIterFn<K, V>)
        }
}

pub trait VecMapKey: Sized {
        fn from(index: usize) -> Self;
        fn get(&self) -> usize;
}

macro_rules! impl_vec_map_key {
        ($t:ident) => {
                impl VecMapKey for $t {
                        fn from(index: usize) -> Self {
                                Self(index)
                        }

                        fn get(&self) -> usize {
                                self.0
                        }
                }

                impl PartialEq for $t {
                        fn eq(&self, other: &Self) -> bool {
                                self.get() == other.get()
                        }
                }

                impl Eq for $t {
                }
        };
}

macro_rules! new_vec_map_keys {
        ($($t:ident),+) => {
                $(
                #[derive(Debug, Clone, Copy)]
                pub struct $t (usize);
                impl_vec_map_key!($t);
                )*
        }
}
