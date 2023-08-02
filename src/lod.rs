use byteorder::{LittleEndian, ReadBytesExt};
use std::convert::TryFrom;
use std::error::Error;
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::PathBuf;

#[allow(dead_code)]
#[derive(Debug)]
struct FileHeader {
    name: String,
    offset: i32,
    size: usize,
    count: i32,
}

const FILE_HEADER_SIZE: usize = 32;
const FILE_INDEX_OFFSET: u64 = 256;

impl TryFrom<&[u8; FILE_HEADER_SIZE]> for FileHeader {
    type Error = Box<dyn Error>;

    fn try_from(data: &[u8; FILE_HEADER_SIZE]) -> Result<Self, Self::Error> {
        let first_zero_idx = data.iter().position(|&x| x == 0).unwrap_or(data.len());
        let name: &str = std::str::from_utf8(&data[0..first_zero_idx])?;

        let mut cursor = Cursor::new(&data[16..]);
        let offset = cursor.read_i32::<LittleEndian>()?;
        let size = cursor.read_i32::<LittleEndian>()?;
        let _ = cursor.read_i32::<LittleEndian>()?;
        let count = cursor.read_i32::<LittleEndian>()?;
        Ok(FileHeader {
            name: name.to_string(),
            offset,
            size: size as usize,
            count,
        })
    }
}

// Enum to represent different versions of the games
pub enum Version {
    MM6,
    MM7,
    MM8,
}

impl TryFrom<&[u8]> for Version {
    type Error = &'static str;

    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        match data {
            b"GameMMVI" | b"MMVI" => Ok(Version::MM6),
            b"GameMMVII" | b"MMVII" => Ok(Version::MM7),
            b"GameMMVIII" | b"MMVIII" => Ok(Version::MM8),
            _ => Err("Invalid game version"),
        }
    }
}

#[allow(dead_code)]
pub struct Lod {
    lod_path: PathBuf,
    pub version: Version,
    files: Vec<FileHeader>,
}

impl Lod {
    pub fn open(file_name: &str) -> Result<Lod, Box<dyn std::error::Error>> {
        let mut file: File = File::open(file_name)?;

        let magic = read_until_zero_byte::<std::io::Error>(&mut file)?;
        let magic = String::from_utf8_lossy(&magic);
        if magic != "LOD" {
            return Err("Invalid file format".into());
        }

        let version =
            Version::try_from(read_until_zero_byte::<std::io::Error>(&mut file)?.as_slice())?;

        file.seek(SeekFrom::Start(FILE_INDEX_OFFSET))?;

        let initial_file_header: FileHeader = read_file_header(&mut file)?;
        let initial_offset = initial_file_header.offset;
        let num_files = initial_file_header.count as usize;
        let mut files = Vec::with_capacity(num_files);
        files.push(initial_file_header);
        for _ in 0..num_files {
            let mut file_header = read_file_header(&mut file)?;
            file_header.offset += initial_offset;
            files.push(file_header);
        }
        Ok(Lod {
            lod_path: file_name.try_into()?,
            version,
            files,
        })
    }

    pub fn files(&self) -> Vec<&str> {
        self.files.iter().map(|f| f.name.as_str()).collect()
    }

    pub fn get<T: TryFrom<Vec<u8>, Error = Box<dyn Error>>>(
        &self,
        name: &str,
    ) -> Result<T, Box<dyn Error>> {
        T::try_from(self.get_raw(name)?)
    }

    pub fn get_raw(&self, name: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        let lf = self.files.iter().find(|f| f.name == name);
        match lf {
            Some(lf) => {
                let mut file: File = File::open(&self.lod_path)?;
                file.seek(SeekFrom::Start(lf.offset as u64))?;
                let mut buf = Vec::new();
                buf.resize(lf.size, 0);
                file.read_exact(&mut buf)?;
                Ok(buf)
            }
            None => Err("file not found!".into()),
        }
    }
}

fn read_file_header(file: &mut File) -> Result<FileHeader, Box<dyn Error>> {
    let mut buf: [u8; FILE_HEADER_SIZE] = [0; FILE_HEADER_SIZE];
    file.read_exact(&mut buf)?;
    let file_header = FileHeader::try_from(&buf)?;
    Ok(file_header)
}

fn read_until_zero_byte<E>(r: &mut dyn Read) -> Result<Vec<u8>, E>
where
    E: From<std::io::Error>,
{
    let mut buffer = Vec::new();
    while let Some(byte) = r.bytes().next() {
        let byte = byte?;
        if byte == 0 {
            break;
        }
        buffer.push(byte);
    }
    Ok(buffer)
}
