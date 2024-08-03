
#[derive(Clone, Copy)]
pub struct FieldVal<'a> {
    offset: u16,
    width: u16,
    data: &'a [u8],
}

impl<'a> FieldVal<'a> {
    pub fn empty() -> Self {
        FieldVal { offset: 0, width: 0, data: &[] }
    }

    pub fn from_slice(data: &'a [u8]) -> Self {
        FieldVal { offset: 0, width: (data.len() * 8) as u16, data }
    }

    pub fn field(&self, offset: u16, width: u16) -> FieldVal<'a> {
        let byte_offset = ((self.offset + offset) / 8) as usize;
        let byte_end = ((self.offset + offset + width + 7) / 8) as usize;

        FieldVal {
            offset: (self.offset + offset) % 8,
            width,
            data: &self.data[byte_offset..byte_end]
        }
    }

    pub fn as_u64(&self) -> u64 {
        assert!(self.width <= 64 && self.data.len() <= 8 && self.offset < 8);
        if self.width == 64 {
            assert!(self.offset == 0 && self.data.len() == 8);
            u64::from_le_bytes(self.data.try_into().unwrap())
        } else {
            let mut e = [0; size_of::<u64>()];
            let n = self.data.len();
            e[..n].copy_from_slice(&self.data[..n]);
            (u64::from_le_bytes(e) >> self.offset) & ((1 << self.width) - 1)
        }
    }

    pub fn eq(&self, o: &Self) -> bool {
        assert!(self.offset == o.offset && self.width == o.width && self.data.len() == o.data.len());
        match self.data.len() {
            0 => true,
            1 => {
                let end_mask = (1 << self.width) - 1;
                let a = (self.data[0] >> self.offset) & end_mask;
                let b = (o.data[0] >> o.offset) & end_mask;
                a == b
            }
            _ => {
                let (a_first, a_rest) = self.data.split_first().unwrap();
                let (b_first, b_rest) = o.data.split_first().unwrap();
                let (a_last, a_mid) = a_rest.split_last().unwrap();
                let (b_last, b_mid) = b_rest.split_last().unwrap();
                let end_mask = (1 << (self.width % 8 - self.offset)) - 1;

                a_first >> self.offset == b_first >> self.offset
                && a_mid == b_mid
                && a_last & end_mask == b_last & end_mask
            }
        }
    }
}