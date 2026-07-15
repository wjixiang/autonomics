//! Single extension trait adding bioinformatics file readers to DataFusion's
//! [`SessionContext`].
//!
//! Every `read_<format>` helper is a thin wrapper over the generic
//! [`read_bio`](crate::datasource::read_bio), specialized on the matching
//! [`BioDriver`](crate::datasource::drivers). This mirrors DataFusion's own
//! `read_csv` / `read_parquet` surface.

use datafusion::execution::context::DataFilePaths;
use datafusion::prelude::{DataFrame, SessionContext};

use crate::datasource::drivers::{
    BamDriver, BcfDriver, BedDriver, BigBedDriver, BigWigDriver, CramDriver, FastaDriver,
    FastqDriver, GffDriver, GtfDriver, SamDriver, VcfDriver,
};
use crate::datasource::{BioReadOptions, read_bio};
use datafusion::common::Result;

/// Extension trait that adds typed readers (`read_vcf`, `read_bam`, …) to a
/// [`SessionContext`], mirroring DataFusion's own `read_csv` / `read_parquet`.
pub trait DataFusionReadExt {
    /// Read VCF file(s) as a queryable [`DataFrame`].
    fn read_vcf<P: DataFilePaths + Send>(
        &self,
        table_paths: P,
        options: BioReadOptions,
    ) -> impl std::future::Future<Output = Result<DataFrame>> + Send;

    /// Read BCF file(s).
    fn read_bcf<P: DataFilePaths + Send>(
        &self,
        table_paths: P,
        options: BioReadOptions,
    ) -> impl std::future::Future<Output = Result<DataFrame>> + Send;

    /// Read FASTA file(s).
    fn read_fasta<P: DataFilePaths + Send>(
        &self,
        table_paths: P,
        options: BioReadOptions,
    ) -> impl std::future::Future<Output = Result<DataFrame>> + Send;

    /// Read FASTQ file(s).
    fn read_fastq<P: DataFilePaths + Send>(
        &self,
        table_paths: P,
        options: BioReadOptions,
    ) -> impl std::future::Future<Output = Result<DataFrame>> + Send;

    /// Read BED file(s).
    fn read_bed<P: DataFilePaths + Send>(
        &self,
        table_paths: P,
        options: BioReadOptions,
    ) -> impl std::future::Future<Output = Result<DataFrame>> + Send;

    /// Read GTF file(s).
    fn read_gtf<P: DataFilePaths + Send>(
        &self,
        table_paths: P,
        options: BioReadOptions,
    ) -> impl std::future::Future<Output = Result<DataFrame>> + Send;

    /// Read GFF file(s).
    fn read_gff<P: DataFilePaths + Send>(
        &self,
        table_paths: P,
        options: BioReadOptions,
    ) -> impl std::future::Future<Output = Result<DataFrame>> + Send;

    /// Read SAM file(s).
    fn read_sam<P: DataFilePaths + Send>(
        &self,
        table_paths: P,
        options: BioReadOptions,
    ) -> impl std::future::Future<Output = Result<DataFrame>> + Send;

    /// Read BAM file(s).
    fn read_bam<P: DataFilePaths + Send>(
        &self,
        table_paths: P,
        options: BioReadOptions,
    ) -> impl std::future::Future<Output = Result<DataFrame>> + Send;

    /// Read CRAM file(s).
    fn read_cram<P: DataFilePaths + Send>(
        &self,
        table_paths: P,
        options: BioReadOptions,
    ) -> impl std::future::Future<Output = Result<DataFrame>> + Send;

    /// Read BigWig file(s).
    fn read_bigwig<P: DataFilePaths + Send>(
        &self,
        table_paths: P,
        options: BioReadOptions,
    ) -> impl std::future::Future<Output = Result<DataFrame>> + Send;

    /// Read BigBed file(s).
    fn read_bigbed<P: DataFilePaths + Send>(
        &self,
        table_paths: P,
        options: BioReadOptions,
    ) -> impl std::future::Future<Output = Result<DataFrame>> + Send;
}

macro_rules! impl_readers {
    ($($method:ident => $driver:ty),* $(,)?) => {
        impl DataFusionReadExt for SessionContext {
            $(
                fn $method<P: DataFilePaths + Send>(
                    &self,
                    table_paths: P,
                    options: BioReadOptions,
                ) -> impl std::future::Future<Output = Result<DataFrame>> + Send {
                    read_bio::<$driver, _>(self, table_paths, options)
                }
            )*
        }
    };
}

impl_readers! {
    read_vcf => VcfDriver,
    read_bcf => BcfDriver,
    read_fasta => FastaDriver,
    read_fastq => FastqDriver,
    read_bed => BedDriver,
    read_gtf => GtfDriver,
    read_gff => GffDriver,
    read_sam => SamDriver,
    read_bam => BamDriver,
    read_cram => CramDriver,
    read_bigwig => BigWigDriver,
    read_bigbed => BigBedDriver,
}
