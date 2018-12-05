use curl::easy::Easy;
use std::io;

use archive_reader::ArchiveBackend;

pub struct RemoteReader {
    url: String,
    handle: curl::easy::Easy,
}

impl RemoteReader {
    pub fn new(url: &str) -> Self {
        let handle = Easy::new();
        RemoteReader {
            url: url.to_string(),
            handle: handle,
        }
    }
}

impl ArchiveBackend for RemoteReader {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        if buf.len() == 0 {
            return Ok(());
        }

        let end_offset = offset + (buf.len() - 1) as u64;

        let mut data = Vec::new();
        self.handle.url(&self.url)?;
        self.handle.range(&format!("{}-{}", offset, end_offset))?;
        {
            let mut transfer = self.handle.transfer();
            transfer.write_function(|new_data| {
                data.extend_from_slice(new_data);
                Ok(new_data.len())
            })?;
            transfer.perform()?;
        }

        for i in 0..data.len() {
            buf[i] = data[i];
        }

        println!(
            "Requested {} bytes, fetched {} bytes",
            buf.len(),
            data.len()
        );
        Ok(())
    }

    fn read_in_chunks<F: FnMut(Vec<u8>)>(
        &mut self,
        start_offset: u64,
        chunk_sizes: &Vec<u64>,
        mut chunk_callback: F,
    ) -> io::Result<()> {
        let tot_size: u64 = chunk_sizes.iter().sum();

        println!(
            "Get {} chunks at offset {}, total size: {}",
            chunk_sizes.len(),
            start_offset,
            tot_size
        );

        // Create get request
        let mut chunk_buf: Vec<u8> = vec![];
        let mut chunk_index = 0;
        let end_offset = start_offset + tot_size - 1;
        self.handle.url(&self.url)?;
        self.handle
            .range(&format!("{}-{}", start_offset, end_offset))?;
        {
            let mut transfer = self.handle.transfer();
            transfer.write_function(|new_data| {
                // Got data back from server
                chunk_buf.extend_from_slice(new_data);

                while chunk_index < chunk_sizes.len()
                    && chunk_buf.len() >= chunk_sizes[chunk_index] as usize
                {
                    // Got a full chunk
                    let chunk_size = chunk_sizes[chunk_index] as usize;
                    chunk_callback(chunk_buf.drain(..chunk_size).collect());
                    chunk_index += 1;
                }
                Ok(new_data.len())
            })?;
            transfer.perform()?;
        }
        Ok(())
    }
}
