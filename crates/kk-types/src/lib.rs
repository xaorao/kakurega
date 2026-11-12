pub mod chat;
pub mod injection;
pub mod label;
pub mod words;

pub const PRODUCT: &str = "kakurega";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_name_is_stable() {
        assert_eq!(PRODUCT, "kakurega");
    }
}
