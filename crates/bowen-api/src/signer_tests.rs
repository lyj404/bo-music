use super::*;

#[test]
fn whatwg_encoding_including_unicode() {
    let pairs = vec![("k".into(), "周杰伦 ~*+&".into())];
    assert_eq!(
        form_url_encode(&pairs),
        "k=%E5%91%A8%E6%9D%B0%E4%BC%A6+%7E*%2B%26"
    );
}
