use byteorder::{LittleEndian, ReadBytesExt};
use std::{
    error::Error,
    io::{Cursor, Seek},
};

use crate::{image, palette, utils};

const SPRITE_HEADER_SIZE: usize = 32;

#[allow(dead_code)]
#[derive(Debug)]
pub struct Sprite {
    height: usize,
    width: usize,
    palette_id: u16,
    data: Vec<u8>,
}

impl TryFrom<Vec<u8>> for Sprite {
    type Error = Box<dyn Error>;

    fn try_from(data: Vec<u8>) -> Result<Self, Self::Error> {
        Self::try_from(data.as_slice())
    }
}

impl TryFrom<&[u8]> for Sprite {
    type Error = Box<dyn Error>;

    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        let mut cursor = Cursor::new(data);
        cursor.seek(std::io::SeekFrom::Start(12))?;

        let compressed_size = cursor.read_u32::<LittleEndian>()? as usize;
        let width = cursor.read_u16::<LittleEndian>()? as usize;
        let height = cursor.read_u16::<LittleEndian>()? as usize;
        let palette_id = cursor.read_u16::<LittleEndian>()?;
        cursor.seek(std::io::SeekFrom::Current(6))?;
        let uncompressed_size = cursor.read_u32::<LittleEndian>()? as usize;

        let table_size: usize = height * 8;
        let table = &data[SPRITE_HEADER_SIZE..(SPRITE_HEADER_SIZE + table_size)];

        let compressed_data = &data[SPRITE_HEADER_SIZE + table_size..];
        utils::check_size(compressed_data.len(), compressed_size)?;

        let uncompressed_data = utils::decompress(compressed_data, uncompressed_size)?;
        utils::check_size(uncompressed_data.len(), uncompressed_size)?;

        let data = process_image_data(height, width, table, uncompressed_data.as_slice())?;

        Ok(Self {
            height,
            width,
            palette_id,
            data,
        })
    }
}

fn process_image_data(
    height: usize,
    width: usize,
    table: &[u8],
    data: &[u8],
) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut img: Vec<u8> = vec![0; width * height];
    let mut img_index = 0;
    let mut cursor = Cursor::new(table);
    for _ in 0..height {
        let start = cursor.read_u16::<LittleEndian>()? as usize;
        let end = cursor.read_u16::<LittleEndian>()? as usize;
        let offset = cursor.read_u32::<LittleEndian>()? as usize;
        img_index += start;
        let data_size = end - start + 1;
        img[img_index..img_index + data_size].copy_from_slice(&data[offset..offset + data_size]);
        img_index += data_size + width - 1 - end;
    }
    Ok(img)
}

impl Sprite {
    pub fn to_png_file(
        &self,
        path: &str,
        palettes: &palette::Palettes,
    ) -> Result<(), Box<dyn Error>> {
        let palette_name = format!("pal{:03}", self.palette_id);
        let palette = palettes
            .map
            .get(&palette_name)
            .ok_or_else(|| "palette not found!".to_string())?;
        let palette = palette.data;

        crate::image::raw_to_image_buffer(
            &self.data,
            &palette,
            self.width as u32,
            self.height as u32,
        )?
        .save_with_format(path, ::image::ImageFormat::Png)?;
        Ok(())
    }
}
