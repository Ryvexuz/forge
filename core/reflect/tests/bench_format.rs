#[path = "../examples/bench.rs"]
mod bench;

#[test]
fn bench_median_rule() {
    assert_eq!(bench::median(&mut [3.0, 1.0, 2.0]), Some(2.0));
    assert_eq!(bench::median(&mut [4.0, 1.0, 3.0, 2.0]), Some(2.5));
    let mut twenty: Vec<_> = (1..=20).rev().map(f64::from).collect();
    assert_eq!(bench::median(&mut twenty), Some(10.5));
    assert_eq!(bench::median(&mut []), None);
    assert_eq!(bench::median(&mut [1.0, f64::NAN]), None);
    assert_eq!(bench::median(&mut [f64::INFINITY]), None);
    assert_eq!(bench::median(&mut [f64::NEG_INFINITY]), None);
    assert_eq!(bench::median(&mut [f64::MAX; 2]), Some(f64::MAX));
    assert_eq!(bench::median(&mut [-f64::MAX, f64::MAX]), Some(0.0));
    let tiny = f64::from_bits(1);
    assert_eq!(bench::median(&mut [tiny; 2]), Some(tiny));
}

fn number(word: &str, prefix: &str, digits: usize) -> f64 {
    let raw = word.strip_prefix(prefix).expect("label");
    let (whole, fraction) = raw.split_once('.').expect("decimal point");
    assert!(!whole.is_empty());
    assert!(whole.bytes().all(|b| b.is_ascii_digit()));
    assert_eq!(fraction.len(), digits);
    assert!(fraction.bytes().all(|b| b.is_ascii_digit()));
    let value = raw.parse::<f64>().expect("number");
    assert!(value.is_finite());
    value
}

#[test]
fn bench_report_lines_have_the_check_format() {
    let [a, b] = bench::report(3, 1000).expect("benchmark");
    let words: Vec<_> = a.split(' ').collect();
    assert_eq!(words.len(), 6);
    assert_eq!(&words[..2], &["bench", "set_path"]);
    assert!(number(words[2], "ns_per_op=", 1) > 0.0);
    assert_eq!(words[3], "direct");
    assert!(number(words[4], "ns_per_op=", 1) > 0.0);
    number(words[5], "ratio=", 2);
    let words: Vec<_> = b.split(' ').collect();
    assert_eq!(words.len(), 3);
    assert_eq!(&words[..2], &["bench", "text_save"]);
    assert!(number(words[2], "ns_per_field=", 1) > 0.0);
    assert!(bench::report(0, 1000).unwrap_err().contains("no median"));
    assert!(bench::report(1, 0).unwrap_err().contains("no median"));
}
