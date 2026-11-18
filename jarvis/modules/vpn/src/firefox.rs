//! Firefox 120 REALITY ClientHello, following the uTLS preset.
use crate::client_hello::{Hello, invalid, u16s, vector};
use reality_tls::Error;

pub(crate) fn hello(original: &[u8]) -> Result<Vec<u8>, Error> {
    let mut hello = Hello::parse(original)?;
    let mut take = |id| hello.extensions.remove(&id).ok_or_else(invalid);
    let shares = take(51)?;
    if shares.len() != 107
        || !shares.starts_with(&[0, 105, 0, 29, 0, 32])
        || shares[38..43] != [0, 23, 0, 65, 4]
    {
        return Err(invalid());
    }

    let mut random = [0; 32 + 239 + 2];
    getrandom::fill(&mut random).map_err(|_| Error::FailedToGetRandomBytes)?;
    let aead = if random[0] & 1 == 0 { 1 } else { 3 };
    let mut ech = vec![0, 0, 1, 0, aead, random[1]];
    ech.extend(vector(&random[2..34]));
    ech.extend(vector(&random[34..]));

    let extensions = vec![
        (0, take(0)?),
        (23, vec![]),
        (65281, vec![0]),
        (10, vector(&u16s(&[29, 23, 24, 25, 256, 257]))),
        (11, vec![1, 0]),
        (35, vec![]),
        (16, take(16)?),
        (5, vec![1, 0, 0, 0, 0]),
        (34, vector(&u16s(&[0x0403, 0x0503, 0x0603, 0x0203]))),
        (51, shares),
        (43, vec![4, 3, 4, 3, 3]),
        (
            13,
            vector(&u16s(&[
                0x0403, 0x0503, 0x0603, 0x0804, 0x0805, 0x0806, 0x0401, 0x0501, 0x0601, 0x0203,
                0x0201,
            ])),
        ),
        (45, vec![1, 1]),
        (28, vec![0x40, 0x01]),
        (65037, ech),
    ];
    let suites = [
        0x1301, 0x1303, 0x1302, 0xc02b, 0xc02f, 0xcca9, 0xcca8, 0xc02c, 0xc030, 0xc00a, 0xc009,
        0xc013, 0xc014, 0x009c, 0x009d, 0x002f, 0x0035,
    ];
    Ok(hello.encode(&suites, extensions))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, reality};
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use x25519_dalek::{PublicKey, StaticSecret};

    #[test]
    fn firefox_profile_offers_both_keys_in_prescribed_extension_order() {
        let key = URL_SAFE_NO_PAD.encode(PublicKey::from(&StaticSecret::from([42; 32])).as_bytes());
        let link = format!(
            "vless://00112233-4455-6677-8899-aabbccddeeff@127.0.0.1:443?security=reality&pbk={key}&sid=&sni=vpn.example.org&flow=xtls-rprx-vision&fp=firefox"
        );
        let config = Config::parse(&link).unwrap();
        assert_eq!(config.info()["fingerprint"], "firefox120");
        let mut client = reality::client(&config).unwrap();
        let mut wire = Vec::new();
        client.write_tls(&mut wire).unwrap();
        let wire = &wire[5..];
        assert_eq!(&wire[73..79], &u16s(&[0x1301, 0x1303, 0x1302]));
        let suite_len = u16::from_be_bytes([wire[71], wire[72]]) as usize;
        let mut extensions = &wire[73 + suite_len + 4..];
        let mut order = Vec::new();
        while !extensions.is_empty() {
            order.push(u16::from_be_bytes([extensions[0], extensions[1]]));
            let len = u16::from_be_bytes([extensions[2], extensions[3]]) as usize;
            extensions = &extensions[4 + len..];
        }
        assert_eq!(
            order,
            [
                0, 23, 65281, 10, 11, 35, 16, 5, 34, 51, 43, 13, 45, 28, 65037
            ]
        );
        let hello = Hello::parse(wire).unwrap();
        let shares = &hello.extensions[&51];
        assert_eq!(shares.len(), 107);
        assert_eq!(&shares[38..43], &[0, 23, 0, 65, 4]);
        assert_eq!(hello.extensions[&65037].len(), 281);
    }
}
