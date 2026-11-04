use kk_secrets::pii::{describe, luhn, scan, verhoeff, Kind};

fn count(text: &str, k: Kind) -> usize {
    scan(text).get(&k).copied().unwrap_or(0)
}

#[test]
fn pi01_each_kind_is_found_in_its_usual_writing() {
    assert_eq!(
        count("write to jane.doe+tag@example.co.in. Thanks", Kind::Email),
        1
    );
    assert_eq!(count("a@b.com and c@d.org", Kind::Email), 2);
    for card in [
        "4111 1111 1111 1111",
        "5555-5555-5555-4444",
        "378282246310005",
        "6011111111111117",
    ] {
        assert_eq!(
            count(&format!("card {card} end"), Kind::PaymentCard),
            1,
            "{card}"
        );
    }
    assert_eq!(count("iban GB82WEST12345698765432.", Kind::Iban), 1);
    assert_eq!(count("DE89370400440532013000", Kind::Iban), 1);
    assert_eq!(count("PAN ABCPE1234F issued", Kind::IndianPan), 1);
    assert_eq!(count("aadhaar 2345 6789 0124", Kind::Aadhaar), 1);
    assert_eq!(count("aadhaar 234567890124", Kind::Aadhaar), 1);
    assert_eq!(count("ssn 123-45-6789", Kind::UsSsn), 1);
    assert_eq!(
        count("12 123-45-6789 and 219-09-9999", Kind::UsSsn),
        2,
        "numbers next to other numbers are still seen"
    );
    for phone in [
        "+91 98765 43210",
        "+1 415 555 2671",
        "415-555-2671",
        "+447911123456",
    ] {
        assert_eq!(
            count(&format!("call {phone} now"), Kind::Phone),
            1,
            "{phone}"
        );
    }
}

#[test]
fn pi02_ordinary_numbers_ids_and_addresses_are_left_alone() {
    for text in [
        "ts=1730736000000",
        "id 20241104123456",
        "host 192.168.100.200 port 8080",
        "ips 10.0.0.1 and 172.16.254.1 and 255.255.255.255",
        "build 2024.11.04.1530 and 1.22.333.4444",
        "version 10.20.30.40",
        "date 2024-01-15 and 2024-01-15 10:30:00",
        "sha 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
        "order 1234567890",
        "total 4,111,111,111,111",
        "ABCDX1234F is not a PAN (fourth letter)",
        "GB82WEST12345698765433 fails the IBAN checksum",
        "ssn 000-12-3456 666-12-3456 900-12-3456 123-00-6789 123-45-0000",
        "aadhaar 234567890125 fails the checksum, 134567890124 starts with 1",
        "user@localhost a@b @handle foo@bar. email@@x.com x@y.z1",
        "4111111111111112 fails Luhn",
        "1111111111111111 is one digit repeated",
    ] {
        assert!(scan(text).is_empty(), "{text}: {:?}", scan(text));
    }
}

#[test]
fn pi03_a_checksummed_number_that_no_card_network_issues_is_not_reported() {
    // Luhn-valid, 16 digits, but no network starts with 91
    let n = (0..10u8)
        .map(|x| 9100000000000000u64 + x as u64)
        .find(|v| luhn(&v.to_string().bytes().map(|b| b - b'0').collect::<Vec<_>>()))
        .unwrap();
    assert!(scan(&format!("n {n}")).is_empty(), "{n}");
}

#[test]
fn pi04_the_checksums_agree_with_independent_values() {
    assert!(luhn(&[4, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]));
    assert!(!luhn(&[4, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2]));
    // 234567890124 was made with a separate Verhoeff implementation (a Python script), not with this code
    let ok: Vec<u8> = "234567890124".bytes().map(|b| b - b'0').collect();
    assert!(verhoeff(&ok));
    let mut bad = ok.clone();
    bad[11] = 5;
    assert!(!verhoeff(&bad));
    let mut swapped = ok.clone();
    swapped.swap(3, 4);
    assert!(!verhoeff(&swapped), "Verhoeff catches a swap of neighbours");
}

#[test]
fn pi05_the_notice_holds_counts_only_and_reads_naturally() {
    let found = scan("a@b.com c@d.org card 4111 1111 1111 1111 ssn 123-45-6789");
    assert_eq!(
        describe(&found),
        "2 email addresses, 1 payment card number, 1 US social security number"
    );
    let text = describe(&found);
    assert!(
        !text.contains("4111") && !text.contains("a@b.com") && !text.contains("123-45"),
        "values never appear"
    );
    assert_eq!(describe(&scan("nothing here")), "");
}

#[test]
fn pi06_hostile_input_finishes_and_stays_bounded() {
    let big =
        "1 ".repeat(200_000) + &"a@".repeat(100_000) + &"A".repeat(100_000) + &"@.".repeat(100_000);
    let t = std::time::Instant::now();
    let _ = scan(&big);
    assert!(t.elapsed().as_secs() < 5, "{:?}", t.elapsed());
    let _ = scan("\u{0}\u{ffff}😀é+@..-@");
}
