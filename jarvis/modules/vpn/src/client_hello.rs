use reality_tls::Error;
use std::collections::BTreeMap;

pub(crate) fn invalid() -> Error {
    Error::General("invalid REALITY ClientHello".into())
}

pub(crate) fn u16s(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

pub(crate) fn vector(bytes: &[u8]) -> Vec<u8> {
    let mut out = (bytes.len() as u16).to_be_bytes().to_vec();
    out.extend(bytes);
    out
}

/// Keep the handshake random and session ID from rustls and replace the public offer.
pub(crate) struct Hello<'a> {
    prefix: &'a [u8],
    pub extensions: BTreeMap<u16, Vec<u8>>,
}

impl<'a> Hello<'a> {
    pub fn parse(original: &'a [u8]) -> Result<Self, Error> {
        if original.len() < 76 || original[0] != 1 || original[38] != 32 {
            return Err(invalid());
        }
        let suite_len = u16::from_be_bytes([original[71], original[72]]) as usize;
        let compression = 73 + suite_len;
        let comp_len = *original.get(compression).ok_or_else(invalid)? as usize;
        let mut raw = original
            .get(compression + 1 + comp_len + 2..)
            .ok_or_else(invalid)?;
        let mut extensions = BTreeMap::new();
        while !raw.is_empty() {
            let head = raw.get(..4).ok_or_else(invalid)?;
            let kind = u16::from_be_bytes([head[0], head[1]]);
            let end = 4 + u16::from_be_bytes([head[2], head[3]]) as usize;
            let body = raw.get(4..end).ok_or_else(invalid)?.to_vec();
            if extensions.insert(kind, body).is_some() {
                return Err(invalid());
            }
            raw = &raw[end..];
        }
        Ok(Self {
            prefix: &original[..71],
            extensions,
        })
    }

    pub fn encode(self, suites: &[u16], extensions: Vec<(u16, Vec<u8>)>) -> Vec<u8> {
        let mut encoded = Vec::new();
        for (kind, body) in extensions {
            encoded.extend(kind.to_be_bytes());
            encoded.extend(vector(&body));
        }
        let mut hello = self.prefix.to_vec();
        hello.extend(vector(&u16s(suites)));
        hello.extend([1, 0]);
        hello.extend(vector(&encoded));
        let length = (hello.len() - 4) as u32;
        hello[1..4].copy_from_slice(&length.to_be_bytes()[1..]);
        hello
    }
}
