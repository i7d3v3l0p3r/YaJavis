//! Chrome 133 REALITY ClientHello profile, following uTLS's BSD-licensed preset.
//! Only REALITY uses this profile; other modules keep their normal TLS implementation.
#[cfg(test)]
mod tests;
use crate::client_hello::{Hello, u16s, vector};
use ml_kem::{EncodedSizeUser, KemCore, MlKem768, kem::Decapsulate};
use reality_tls::{
    Error, NamedGroup,
    crypto::{ActiveKeyExchange, SharedSecret, SupportedKxGroup},
};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::{Zeroize, Zeroizing};

fn invalid() -> Error {
    Error::General("invalid Chrome REALITY hello or key share".into())
}
#[derive(Debug)]
pub(crate) struct HybridGroup;
pub(crate) static HYBRID: HybridGroup = HybridGroup;
impl SupportedKxGroup for HybridGroup {
    fn name(&self) -> NamedGroup {
        NamedGroup::X25519MLKEM768
    }
    fn start(&self) -> Result<Box<dyn ActiveKeyExchange>, Error> {
        Ok(Box::new(Exchange::new()?))
    }
    fn usable_for_version(&self, version: reality_tls::ProtocolVersion) -> bool {
        version == reality_tls::ProtocolVersion::TLSv1_3
    }
}
pub(crate) struct Exchange {
    secret: StaticSecret,
    xpublic: [u8; 32],
    kem: <MlKem768 as KemCore>::DecapsulationKey,
    public: Vec<u8>,
}
impl Exchange {
    pub fn new() -> Result<Self, Error> {
        let mut random = Zeroizing::new([0; 96]);
        getrandom::fill(random.as_mut()).map_err(|_| Error::FailedToGetRandomBytes)?;
        let secret =
            StaticSecret::from(<[u8; 32]>::try_from(&random[..32]).map_err(|_| invalid())?);
        let xpublic = PublicKey::from(&secret).to_bytes();
        let mut d: ml_kem::B32 = <[u8; 32]>::try_from(&random[32..64])
            .map_err(|_| invalid())?
            .into();
        let mut z: ml_kem::B32 = <[u8; 32]>::try_from(&random[64..])
            .map_err(|_| invalid())?
            .into();
        let (kem, ek) = MlKem768::generate_deterministic(&d, &z);
        d.as_mut_slice().zeroize();
        z.as_mut_slice().zeroize();
        let mut public = ek.as_bytes().to_vec();
        public.extend(xpublic);
        Ok(Self {
            secret,
            xpublic,
            kem,
            public,
        })
    }
    pub fn reality_secret(&self, server: [u8; 32]) -> Result<Zeroizing<[u8; 32]>, Error> {
        let shared = self.secret.diffie_hellman(&PublicKey::from(server));
        if !shared.was_contributory() {
            return Err(invalid());
        }
        Ok(Zeroizing::new(shared.to_bytes()))
    }
    fn classical(&self, peer: &[u8]) -> Result<Zeroizing<[u8; 32]>, Error> {
        self.reality_secret(peer.try_into().map_err(|_| invalid())?)
    }
}
impl ActiveKeyExchange for Exchange {
    fn group(&self) -> NamedGroup {
        NamedGroup::X25519MLKEM768
    }
    fn pub_key(&self) -> &[u8] {
        &self.public
    }
    fn hybrid_component(&self) -> Option<(NamedGroup, &[u8])> {
        Some((NamedGroup::X25519, &self.xpublic))
    }
    fn complete_hybrid_component(self: Box<Self>, peer: &[u8]) -> Result<SharedSecret, Error> {
        Ok(SharedSecret::from(self.classical(peer)?.as_slice()))
    }
    fn complete(self: Box<Self>, peer: &[u8]) -> Result<SharedSecret, Error> {
        if peer.len() != 1088 + 32 {
            return Err(invalid());
        }
        let ciphertext = peer[..1088].try_into().map_err(|_| invalid())?;
        let classical = self.classical(&peer[1088..])?;
        let mut kem = self.kem.decapsulate(ciphertext).map_err(|_| invalid())?;
        let mut secret = Zeroizing::new([0; 64]);
        secret[..32].copy_from_slice(&kem);
        secret[32..].copy_from_slice(&classical[..]);
        kem.as_mut_slice().zeroize();
        Ok(SharedSecret::from(secret.as_slice()))
    }
}

fn grease(byte: u8) -> u16 {
    let b = (byte & 15) * 16 + 10;
    u16::from_be_bytes([b, b])
}

/// Preserve the handshake's cryptographic inputs; shape only the browser's public offer.
pub(crate) fn hello(original: &[u8]) -> Result<Vec<u8>, Error> {
    let mut hello = Hello::parse(original)?;
    let shares = hello.extensions.get(&51).ok_or_else(invalid)?;
    // Real hybrid and classical shares must be present. Never advertise a fake PQ key.
    if shares.len() != 2 + 4 + 1216 + 4 + 32 {
        return Err(invalid());
    }
    let mut random = [0; 256];
    getrandom::fill(&mut random).map_err(|_| Error::FailedToGetRandomBytes)?;
    let (cipher_grease, group_grease, version_grease) =
        (grease(random[0]), grease(random[1]), grease(random[2]));
    let first_grease = grease(random[3]);
    let mut last_grease = grease(random[4]);
    if first_grease == last_grease {
        last_grease = grease(random[4].wrapping_add(1));
    }
    let mut keys = u16s(&[group_grease, 1]);
    keys.push(0);
    keys.extend(&shares[2..]);
    let mut versions = vec![6];
    versions.extend(u16s(&[version_grease, 0x0304, 0x0303]));
    let ech_payload_len = [128, 160, 192, 224][random[5] as usize % 4];
    let mut ech = vec![0, 0, 1, 0, 1, random[6], 0, 32];
    ech.extend(&random[16..48]);
    ech.extend((ech_payload_len as u16).to_be_bytes());
    let mut ech_payload = vec![0; ech_payload_len];
    getrandom::fill(&mut ech_payload).map_err(|_| Error::FailedToGetRandomBytes)?;
    ech.extend(ech_payload);
    let mut extensions: Vec<(u16, Vec<u8>)> = vec![
        (0, hello.extensions.remove(&0).ok_or_else(invalid)?), // SNI
        (23, vec![]),
        (65281, vec![0]),
        (10, vector(&u16s(&[group_grease, 4588, 29, 23, 24]))),
        (11, vec![1, 0]),
        (35, vec![]),
        (
            16,
            vec![
                0, 12, 2, b'h', b'2', 8, b'h', b't', b't', b'p', b'/', b'1', b'.', b'1',
            ],
        ),
        (5, vec![1, 0, 0, 0, 0]),
        (
            13,
            vector(&u16s(&[
                0x0403, 0x0804, 0x0401, 0x0503, 0x0805, 0x0501, 0x0806, 0x0601,
            ])),
        ),
        (18, vec![]),
        (51, vector(&keys)),
        (45, vec![1, 1]),
        (43, versions),
        (27, vec![2, 0, 2]), // genuine Brotli decoder enabled in REALITY client config
        (17613, vec![0, 3, 2, b'h', b'2']), // Chrome 133 ALPS extension
        (65037, ech),
    ];
    // Fisher-Yates with rejection sampling avoids a fixed extension ordering fingerprint.
    for i in (1..extensions.len()).rev() {
        let bound = (i + 1) as u32;
        let ceiling = u32::MAX - u32::MAX % bound;
        let index = loop {
            let mut bytes = [0; 4];
            getrandom::fill(&mut bytes).map_err(|_| Error::FailedToGetRandomBytes)?;
            let n = u32::from_le_bytes(bytes);
            if n < ceiling {
                break (n % bound) as usize;
            }
        };
        extensions.swap(i, index);
    }
    extensions.insert(0, (first_grease, vec![]));
    extensions.push((last_grease, vec![0]));
    let mut suites = vec![cipher_grease];
    suites.extend([
        0x1301, 0x1302, 0x1303, 0xc02b, 0xc02f, 0xc02c, 0xc030, 0xcca9, 0xcca8, 0xc013, 0xc014,
        0x009c, 0x009d, 0x002f, 0x0035,
    ]);
    Ok(hello.encode(&suites, extensions))
}
