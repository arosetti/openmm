use byteorder::{LittleEndian, ReadBytesExt};
use image::{imageops, DynamicImage, GenericImageView, ImageBuffer, Rgba};
use std::{
    error::Error,
    io::{Cursor, Seek},
    path::Path,
};

use super::{palette::Palettes, zlib};
use crate::LodManager;

#[derive(Debug)]
pub(super) struct Image {
    pub height: usize,
    pub width: usize,
    pub data: Vec<u8>,
    pub palette: [u8; PALETTE_SIZE],
    pub transparency: bool,
}

const PALETTE_SIZE: usize = 256 * 3;
const BITMAP_HEADER_SIZE: usize = 48;
const SPRITE_HEADER_SIZE: usize = 32;

/// This is for bitmap images
impl TryFrom<&[u8]> for Image {
    type Error = Box<dyn Error>;

    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        let mut cursor = Cursor::new(data);
        cursor.seek(std::io::SeekFrom::Start(16))?;
        let pixel_size = cursor.read_u32::<LittleEndian>()? as usize;
        let compressed_size = cursor.read_u32::<LittleEndian>()? as usize;
        let width = cursor.read_u16::<LittleEndian>()? as usize;
        let height = cursor.read_u16::<LittleEndian>()? as usize;
        cursor.seek(std::io::SeekFrom::Current(12))?;
        let uncompressed_size = cursor.read_u32::<LittleEndian>()? as usize;

        if pixel_size == 0 {
            return Err("Pixel size is zero, this is not a valid image".into());
        }
        if data.len() <= BITMAP_HEADER_SIZE + PALETTE_SIZE {
            return Err("Not enough data".into());
        }

        let compressed_data = &data[BITMAP_HEADER_SIZE..data.len() - PALETTE_SIZE];
        let uncompressed_data =
            zlib::decompress(compressed_data, compressed_size, uncompressed_size)?;

        let palette_slice = &data[data.len() - PALETTE_SIZE..];
        let palette: [u8; PALETTE_SIZE] = palette_slice.try_into()?;

        Ok(Self {
            height,
            width,
            data: uncompressed_data,
            palette,
            transparency: false,
        })
    }
}

/// This is for sprite images
impl TryFrom<(&[u8], &Palettes)> for Image {
    type Error = Box<dyn Error>;

    fn try_from(data: (&[u8], &Palettes)) -> Result<Self, Self::Error> {
        let palettes = data.1;
        let data = data.0;

        let mut cursor = Cursor::new(data);
        cursor.seek(std::io::SeekFrom::Start(12))?;

        let compressed_size = cursor.read_u32::<LittleEndian>()? as usize;
        let width = cursor.read_u16::<LittleEndian>()? as usize;
        let height = cursor.read_u16::<LittleEndian>()? as usize;

        let palette_id = cursor.read_u16::<LittleEndian>()?;
        let palette = palettes
            .get(palette_id)
            .ok_or_else(|| "Palette not found!".to_string())?;

        cursor.seek(std::io::SeekFrom::Current(6))?;
        let uncompressed_size = cursor.read_u32::<LittleEndian>()? as usize;

        let table_size: usize = height * 8;

        if data.len() <= SPRITE_HEADER_SIZE + table_size {
            return Err("Not enough data".into());
        }

        let table = &data[SPRITE_HEADER_SIZE..(SPRITE_HEADER_SIZE + table_size)];

        let compressed_data = &data[SPRITE_HEADER_SIZE + table_size..];
        let uncompressed_data =
            super::zlib::decompress(compressed_data, compressed_size, uncompressed_size)?;

        let processed_data =
            process_sprite_data(uncompressed_data.as_slice(), table, width, height)?;

        Ok(Self {
            height,
            width,
            data: processed_data,
            palette: palette.data,
            transparency: true,
        })
    }
}

fn process_sprite_data(
    data: &[u8],
    table: &[u8],
    width: usize,
    height: usize,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let img_size = width * height;
    let mut img: Vec<u8> = vec![0; img_size];
    let mut current: usize = 0;
    let mut cursor = Cursor::new(table);

    for _ in 0..height {
        let start = cursor.read_i16::<LittleEndian>()?;
        let end = cursor.read_i16::<LittleEndian>()?;
        let offset = cursor.read_u32::<LittleEndian>()? as usize;

        if start < 0 || end < 0 {
            current += width - 1;
            continue;
        }

        current += start as usize;
        let chunk_size = (end - start + 1) as usize;
        img[current..current + chunk_size].copy_from_slice(&data[offset..offset + chunk_size]);
        current += width - start as usize;
    }
    Ok(img)
}

impl Image {
    pub fn to_image_buffer(&self) -> Result<DynamicImage, Box<dyn Error>> {
        let image = raw_to_image_buffer(
            &self.data,
            &self.palette,
            |index, pixel: &[u8; 3]| {
                if self.transparency && index == self.data[0] {
                    Rgba([0, 0, 0, 0])
                } else {
                    Rgba([pixel[0], pixel[1], pixel[2], 255])
                }
            },
            self.width as u32,
            self.height as u32,
        )?;
        Ok(DynamicImage::ImageRgba8(image))
    }

    #[allow(dead_code)]
    pub fn save<Q>(&self, path: Q) -> Result<(), Box<dyn Error>>
    where
        Q: AsRef<Path>,
    {
        self.to_image_buffer()?
            .save_with_format(path, image::ImageFormat::Png)?;
        Ok(())
    }
}

/// Converts the image into a versatile generic image buffer.
/// The image contains more pixels than needed with dimensions (h*w) to account for mipmaps,
/// but we are currently not utilizing those extra pixels.
/// # Panics
/// if the input accesses outside the bounds of the palette.
fn raw_to_image_buffer<P>(
    data: &[u8],
    palette: &[u8; 768],
    pixel_converter: impl Fn(u8, &[u8; 3]) -> P,
    width: u32,
    height: u32,
) -> Result<ImageBuffer<P, Vec<P::Subpixel>>, Box<dyn Error>>
where
    P: image::Pixel<Subpixel = u8> + 'static,
{
    let mut image_buffer = ImageBuffer::<P, Vec<P::Subpixel>>::new(width, height);

    for (i, pi) in data[..(width * height) as usize].iter().enumerate() {
        let x = (i as u32).rem_euclid(width);
        let y = (i as u32).div_euclid(width);
        let index = 3 * (*pi as usize);
        let pixel = pixel_converter(*pi, &palette[index..index + 3].try_into()?);
        image_buffer.put_pixel(x, y, pixel);
    }
    Ok(image_buffer)
}

fn join_images_in_grid(
    images: &[DynamicImage],
    grid_width: usize,
    image_width: u32,
    image_height: u32,
) -> DynamicImage {
    let num_images = images.len();
    if num_images == 0 {
        panic!("No images provided.");
    }

    let combined_width = image_width * grid_width as u32;
    let combined_height = image_height * ((num_images as f32 / grid_width as f32).ceil() as u32);

    let mut combined_image = ImageBuffer::new(combined_width, combined_height);

    for (i, image) in images.iter().enumerate() {
        let x_offset = (i % grid_width) as u32 * image_width;
        let y_offset = (i / grid_width) as u32 * image_height;
        for y in 0..image_height {
            for x in 0..image_width {
                let pixel = image.get_pixel(x, y);
                if pixel.0[0..2] != [0, 255, 255] {
                    combined_image.put_pixel(x + x_offset, y + y_offset, pixel);
                }
            }
        }
    }
    DynamicImage::ImageRgba8(combined_image)
}

pub fn get_atlas(
    lod_manager: &LodManager,
    names: &[&str],
    row_size: usize,
) -> Result<DynamicImage, Box<dyn Error>> {
    let mut images: Vec<DynamicImage> = Vec::with_capacity(names.len());

    // HACK instead of using shaders I'll compose water in texture gen. :(
    let image_water = lod_manager.bitmap("wtrtyl").ok_or("image not found")?;

    for name in names {
        let mut image = lod_manager.bitmap(name).ok_or("image not found")?;
        if image.dimensions() != (128, 128) {
            image = DynamicImage::ImageRgba8(imageops::resize(
                &image,
                128,
                128,
                imageops::FilterType::Triangle,
            ));
        }

        let image_buffer = image.as_mut_rgba8().ok_or("wrong image format")?;
        for y in 0..128 {
            for x in 0..128 {
                let rgb: [u8; 4] = image_buffer.get_pixel(x, y).0;
                if rgb[0] == 0 && rgb[1] >= 252 && rgb[2] >= 252 {
                    image_buffer.put_pixel(x, y, image_water.get_pixel(x, y));
                }
            }
        }

        images.push(image);
    }
    Ok(join_images_in_grid(&images, row_size, 128, 128))
}

#[cfg(test)]
mod test {
    use super::get_atlas;
    use crate::{get_lod_path, LodManager};
    use image::GenericImageView;

    #[test]
    fn join_images() {
        let lod_path = get_lod_path();
        let lod_manager = LodManager::new(lod_path).unwrap();

        let atlas_image = get_atlas(
            &lod_manager,
            &["grastyl", "dirttyl", "voltyl", "wtrtyl", "pending"],
            2,
        )
        .unwrap();
        assert_eq!(atlas_image.dimensions(), (128 * 2, 128 * 3));
    }
}
