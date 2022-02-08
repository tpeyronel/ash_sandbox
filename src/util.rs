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
