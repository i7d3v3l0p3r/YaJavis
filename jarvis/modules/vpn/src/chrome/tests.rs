use super::*;
use crate::{config::Config, reality};
use ml_kem::kem::Encapsulate;
use std::collections::{BTreeMap, BTreeSet};

fn wire() -> Vec<u8> {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let key = URL_SAFE_NO_PAD.encode(PublicKey::from(&StaticSecret::from([42; 32])).as_bytes());
    let config=Config::parse(&format!("vless://00112233-4455-6677-8899-aabbccddeeff@127.0.0.1:443?security=reality&pbk={key}&sid=0123456789abcdef&sni=vpn.example.org&fp=chrome")).unwrap();
    assert_eq!(config.info()["fingerprint"], "chrome133");
    let mut io = reality::client(&config).unwrap();
    let mut wire = Vec::new();
    io.write_tls(&mut wire).unwrap();
    wire[5..].to_vec()
}
#[test]
fn chrome133_offer_matches_utls_profile_and_permutates_extensions() {
    let is_grease = |value: u16| value & 0x0f0f == 0x0a0a && value >> 8 == value & 255;
    let mut orders = BTreeSet::new();
    for _ in 0..8 {
        let hello = wire();
        assert_eq!(
            u32::from_be_bytes([0, hello[1], hello[2], hello[3]]) as usize,
            hello.len() - 4
        );
        assert_eq!(&hello[4..6], &[3, 3]);
        assert_eq!(hello[38], 32);
        let suite_len = u16::from_be_bytes([hello[71], hello[72]]) as usize;
        let suites: Vec<u16> = hello[73..73 + suite_len]
            .chunks_exact(2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
            .collect();
        assert!(is_grease(suites[0]));
        assert_eq!(
            &suites[1..],
            &[
                0x1301, 0x1302, 0x1303, 0xc02b, 0xc02f, 0xc02c, 0xc030, 0xcca9, 0xcca8, 0xc013,
                0xc014, 0x009c, 0x009d, 0x002f, 0x0035
            ]
        );
        let mut rest = &hello[73 + suite_len..];
        assert_eq!(&rest[..2], &[1, 0]);
        rest = &rest[2..];
        let len = u16::from_be_bytes([rest[0], rest[1]]) as usize;
        assert_eq!(len, rest.len() - 2);
        rest = &rest[2..];
        let mut extensions = BTreeMap::new();
        let mut order = Vec::new();
        while !rest.is_empty() {
            let kind = u16::from_be_bytes([rest[0], rest[1]]);
            let len = u16::from_be_bytes([rest[2], rest[3]]) as usize;
            assert!(extensions.insert(kind, rest[4..4 + len].to_vec()).is_none());
            order.push(kind);
            rest = &rest[4 + len..];
        }
        assert!(is_grease(order[0]) && is_grease(*order.last().unwrap()));
        assert_ne!(order[0], *order.last().unwrap());
        assert_eq!(extensions[&order[0]], Vec::<u8>::new());
        assert_eq!(extensions[order.last().unwrap()], vec![0]);
        let real: BTreeSet<_> = order.iter().copied().filter(|k| !is_grease(*k)).collect();
        assert_eq!(
            real,
            [
                0, 5, 10, 11, 13, 16, 18, 23, 27, 35, 43, 45, 51, 17613, 65037, 65281
            ]
            .into_iter()
            .collect()
        );
        assert_eq!(
            extensions[&13],
            vector(&u16s(&[
                0x0403, 0x0804, 0x0401, 0x0503, 0x0805, 0x0501, 0x0806, 0x0601
            ]))
        );
        assert_eq!(extensions[&27], vec![2, 0, 2]);
        assert_eq!(extensions[&17613], vec![0, 3, 2, b'h', b'2']);
        assert_eq!(extensions[&45], vec![1, 1]);
        assert_eq!(extensions[&11], vec![1, 0]);
        let groups = &extensions[&10];
        assert!(is_grease(u16::from_be_bytes([groups[2], groups[3]])));
        assert_eq!(&groups[4..], u16s(&[4588, 29, 23, 24]));
        let keys = &extensions[&51];
        assert_eq!(&keys[2..4], &groups[2..4]);
        assert_eq!(&keys[4..11], &[0, 1, 0, 0x11, 0xec, 4, 0xc0]);
        assert_eq!(&keys[1227..1231], &[0, 29, 0, 32]);
        assert_eq!(&keys[1195..1227], &keys[1231..1263]);
        let ech = &extensions[&65037];
        assert_eq!(&ech[..5], &[0, 0, 1, 0, 1]);
        assert_eq!(&ech[6..8], &[0, 32]);
        assert!([128, 160, 192, 224].contains(&(ech.len() - 42)));
        orders.insert(order[1..order.len() - 1].to_vec());
    }
    assert!(
        orders.len() > 1,
        "Chrome extension order must vary between connections"
    );
}

#[test]
fn chrome_profile_rejects_conflicting_alpn_and_other_fingerprints() {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let key=URL_SAFE_NO_PAD.encode(PublicKey::from(&StaticSecret::from([42;32])).as_bytes());
    let base=format!("vless://00112233-4455-6677-8899-aabbccddeeff@127.0.0.1:443?security=reality&pbk={key}&sni=vpn.example.org");
    assert!(Config::parse(&format!("{base}&fp=chrome&alpn=h2%2Chttp%2F1.1")).is_ok());
    assert!(Config::parse(&format!("{base}&fp=chrome&alpn=http%2F1.1")).is_err());
    assert!(Config::parse(&format!("{base}&fp=safari")).is_err());
}
#[test]
fn hybrid_and_classical_secrets_match_independent_peer_and_reject_low_order_keys() {
    let exchange = Exchange::new().unwrap();
    let peer = StaticSecret::from([71; 32]);
    let peer_public = PublicKey::from(&peer);
    let ek = <MlKem768 as KemCore>::EncapsulationKey::from_bytes(
        (&exchange.public[..1184]).try_into().unwrap(),
    );
    let (ciphertext, kem_shared) = ek.encapsulate(&mut rand_core::OsRng).unwrap();
    let classical = peer.diffie_hellman(&PublicKey::from(exchange.xpublic));
    let mut response = ciphertext.to_vec();
    response.extend(peer_public.as_bytes());
    let result = Box::new(exchange).complete(&response).unwrap();
    let mut expected = kem_shared.to_vec();
    expected.extend(classical.as_bytes());
    assert_eq!(result.secret_bytes(), expected);
    let exchange = Exchange::new().unwrap();
    let expected = peer.diffie_hellman(&PublicKey::from(exchange.xpublic));
    assert_eq!(
        Box::new(exchange)
            .complete_hybrid_component(peer_public.as_bytes())
            .unwrap()
            .secret_bytes(),
        expected.as_bytes()
    );
    assert!(
        Box::new(Exchange::new().unwrap())
            .complete(&[0; 1120])
            .is_err()
    );
    assert!(
        Box::new(Exchange::new().unwrap())
            .complete_hybrid_component(&[0; 32])
            .is_err()
    );
}
