use std::ops::Deref;

#[allow(unused)]
pub fn log_error<E: std::error::Error>(e: E) {
        log::error!("{}", e);
}

#[allow(unused)]
pub fn log_if_error<T, E: std::error::Error>(r: Result<T, E>) {
        if let Err(e) = r {
                log::error!("{}", e);
        }
}

pub trait RefIntoSlice {
        fn ref_into_slice(&self) -> &[Self]
        where
                Self: Sized;
}

impl<T> RefIntoSlice for T {
        fn ref_into_slice(&self) -> &[Self] {
                std::slice::from_ref(self)
        }
}

pub trait DerefIntoSlice<D> {
        fn deref_into_slice(&self) -> &[D]
        where
                D: Sized;
}

impl<T: Deref<Target = D>, D> DerefIntoSlice<D> for T {
        fn deref_into_slice(&self) -> &[D] {
                std::slice::from_ref(&**self)
        }
}
