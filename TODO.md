### Should

### Probably

 * On compress - Allow for fixed block size.

### Maybe

 * On unpack - Add optional flag to avoid writing destination if the unpacked chunk and the destination data is exactly the same. This as the write speed on some block devices is alot lower than the read speed. And to avoid unnecessary block wear when writing large devices/partitions.

 * Option to generate chunk cache on unpack. A file containing a chunk dictionary but refering to chunk data in the unpacked target file. The cache could be used when seeding next time to avoid having to scan for chunks.