use alloc::vec::Vec;
use ark_ff::FftField;
use ark_ff::Field;
use ark_ff::One;
use ark_ff::Zero;
use ark_poly::domain::Radix2EvaluationDomain;
use ark_poly::EvaluationDomain;
use ark_serialize::CanonicalDeserialize;
use ark_serialize::CanonicalSerialize;
use ark_serialize::Valid;
use core::alloc::AllocError;
use core::alloc::Allocator;
use core::alloc::Layout;
use core::fmt::Display;
use core::ops::Add;
use core::ops::AddAssign;
use core::ops::Div;
use core::ops::Mul;
use core::ops::Neg;
use core::ptr::NonNull;
use num_traits::Pow;
#[cfg(feature = "parallel")]
use rayon::prelude::*;
use std::iter::zip;

#[cfg(feature = "std")]
pub struct Timer<'a> {
    name: &'a str,
    start: std::time::Instant,
}

#[cfg(feature = "std")]
impl<'a> Timer<'a> {
    pub fn new(name: &'a str) -> Timer<'a> {
        let start = std::time::Instant::now();
        Timer { name, start }
    }
}

#[cfg(feature = "std")]
impl<'a> Drop for Timer<'a> {
    fn drop(&mut self) {
        println!("{} in {:?}", self.name, self.start.elapsed());
    }
}

pub fn interleave<T: Copy + Send + Sync + Default, const RADIX: usize>(
    source: &[T],
) -> Vec<[T; RADIX]> {
    let n = source.len() / RADIX;
    let mut res = vec![[T::default(); RADIX]; n];
    ark_std::cfg_iter_mut!(res)
        .enumerate()
        .for_each(|(i, element)| {
            for j in 0..RADIX {
                element[j] = source[i + j * n];
            }
        });
    res
}

// pub(crate) fn print_row<F: Field>(row: &[F]) {
//     for val in row {
//         print!("{val}, ");
//     }
//     println!()
// }

/// Rounds the input value up the the nearest power of two
pub const fn ceil_power_of_two(value: usize) -> usize {
    if value.is_power_of_two() {
        value
    } else {
        value.next_power_of_two()
    }
}

// from arkworks
/// This evaluates the vanishing polynomial for this domain at tau.
pub fn evaluate_vanishing_polynomial<F: FftField + Into<T>, T: Field>(
    domain: &Radix2EvaluationDomain<F>,
    tau: T,
) -> T {
    tau.pow([domain.size() as u64]) - domain.coset_offset_pow_size().into()
}

// Evaluates the vanishing polynomial for `vanish_domain` over `eval_domain`
// E.g. evaluates `(x - v_0)(x - v_1)...(x - v_n-1)` over `eval_domain`
pub fn fill_vanishing_polynomial<F: FftField>(
    dst: &mut [F],
    vanish_domain: &Radix2EvaluationDomain<F>,
    eval_domain: &Radix2EvaluationDomain<F>,
) {
    let n = vanish_domain.size();
    let scaled_eval_offset = eval_domain.coset_offset().pow([n as u64]);
    let scaled_eval_generator = eval_domain.group_gen().pow([n as u64]);
    let scaled_vanish_offset = vanish_domain.coset_offset_pow_size();

    #[cfg(feature = "parallel")]
    let chunk_size = core::cmp::max(n / rayon::current_num_threads(), 1024);
    #[cfg(not(feature = "parallel"))]
    let chunk_size = n;

    ark_std::cfg_chunks_mut!(dst, chunk_size)
        .enumerate()
        .for_each(|(i, chunk)| {
            let mut acc = scaled_eval_offset * scaled_eval_generator.pow([(i * chunk_size) as u64]);
            for coeff in chunk.iter_mut() {
                *coeff = acc - scaled_vanish_offset;
                acc *= &scaled_eval_generator;
            }
        });
}

// taken from arkworks-rs
/// Horner's method for polynomial evaluation
#[inline]
pub fn horner_evaluate<F: Field, T: Field + for<'a> Add<&'a F, Output = T>>(
    poly_coeffs: &[F],
    point: &T,
) -> T {
    poly_coeffs
        .iter()
        .rfold(T::zero(), move |result, coeff| result * point + coeff)
}

/// Calculates `c * (P(X) - P(z)) / (x^a - z)` using synthetic division
/// <https://en.wikipedia.org/wiki/Synthetic_division>
// code taken from OpenZKP
pub fn divide_out_point_into<
    Fp: Field,
    Fq: Field + for<'a> AddAssign<&'a Fp> + for<'a> Mul<&'a Fp>,
>(
    dst_coeffs: &mut [Fq],
    src_coeffs: &[Fp],
    z: &Fq,
    c: &Fq,
) {
    let mut remainder = Fq::zero();
    for (coefficient, target) in zip(src_coeffs, dst_coeffs).rev() {
        // TODO: see if there is a perf difference using references
        *target += remainder * c;
        remainder *= z;
        remainder += coefficient;
    }
}

// TODO: change name/add description
const GRINDING_CONTRIBUTION_FLOOR: usize = 80;

// taken from Winterfell
// also https://github.com/starkware-libs/ethSTARK/blob/master/README.md#7-Measuring-Security
// https://eprint.iacr.org/2020/654.pdf section 7.2 for proven security
// TODO: must investigate and confirm all this.
// TODO: determine if
pub fn conjectured_security_level(
    field_bits: usize,
    hash_fn_security: usize,
    lde_blowup_factor: usize,
    trace_len: usize,
    num_fri_quiries: usize,
    grinding_factor: usize,
) -> usize {
    // compute max security we can get for a given field size
    let field_security = field_bits - (lde_blowup_factor * trace_len).trailing_zeros() as usize;

    // compute security we get by executing multiple query rounds
    let security_per_query = lde_blowup_factor.ilog2() as usize;
    let mut query_security = security_per_query * num_fri_quiries;

    // include grinding factor contributions only for proofs adequate security
    if query_security >= GRINDING_CONTRIBUTION_FLOOR {
        query_security += grinding_factor;
    }

    core::cmp::min(
        core::cmp::min(field_security, query_security) - 1,
        hash_fn_security,
    )
}

// TODO: docs
pub fn reduce_lde_blowup_factor<T: Copy>(
    lde: &mut GpuVec<T>,
    blowup_from: usize,
    blowup_to: usize,
) {
    assert!(blowup_to <= blowup_from);
    assert!(blowup_from.is_power_of_two());
    assert!(blowup_to.is_power_of_two());
    let reduction_factor = blowup_from / blowup_to;

    if reduction_factor == 1 {
        return;
    }

    for i in 0..lde.len() / reduction_factor {
        lde[i] = lde[i * reduction_factor];
    }

    lde.truncate(lde.len() / reduction_factor);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FieldType {
    Fp,
    Fq,
}

impl CanonicalSerialize for FieldType {
    fn serialize_with_mode<W: ark_serialize::Write>(
        &self,
        writer: W,
        compress: ark_serialize::Compress,
    ) -> Result<(), ark_serialize::SerializationError> {
        (*self as u8).serialize_with_mode(writer, compress)
    }

    fn serialized_size(&self, _compress: ark_serialize::Compress) -> usize {
        1
    }
}

impl Valid for FieldType {
    fn check(&self) -> Result<(), ark_serialize::SerializationError> {
        Ok(())
    }
}

impl CanonicalDeserialize for FieldType {
    fn deserialize_with_mode<R: ark_serialize::Read>(
        reader: R,
        compress: ark_serialize::Compress,
        validate: ark_serialize::Validate,
    ) -> Result<Self, ark_serialize::SerializationError> {
        let x = u8::deserialize_with_mode(reader, compress, validate)?;
        Ok(if x == Self::Fp as u8 {
            Self::Fp
        } else if x == Self::Fq as u8 {
            Self::Fq
        } else {
            unreachable!()
        })
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum FieldVariant<Fp, Fq> {
    Fp(Fp),
    Fq(Fq),
}

macro_rules! map {
    ($self:expr, $f1:ident $(, $x:expr)*) => {
        match $self {
            FieldVariant::Fp(v) => FieldVariant::Fp(v.$f1($($x)*)),
            FieldVariant::Fq(v) => FieldVariant::Fq(v.$f1($($x)*)),
        }
    }
}

// impl<Fp: Ord, Fq: Ord> Ord

impl<Fp: Field, Fq: Field> FieldVariant<Fp, Fq> {
    /// Computes the multiplicative inverse of `self` if `self` is nonzero.
    #[inline]
    pub fn inverse(&self) -> Option<Self> {
        match self {
            Self::Fp(v) => v.inverse().map(|v| Self::Fp(v)),
            Self::Fq(v) => v.inverse().map(|v| Self::Fq(v)),
        }
    }

    /// Exponentiates this element by a number represented with `u64` limbs,
    /// least significant limb first.
    #[inline]
    pub fn pow<S: AsRef<[u64]>>(&self, exp: S) -> Self {
        map!(self, pow, exp)
    }

    #[inline]
    pub fn as_fq(&self) -> Fq
    where
        Fq: From<Fp>,
    {
        match self {
            Self::Fp(v) => Fq::from(*v),
            Self::Fq(v) => *v,
        }
    }
}

impl<Fp: Display, Fq: Display> Display for FieldVariant<Fp, Fq> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Fp(v) => Display::fmt(v, f),
            Self::Fq(v) => Display::fmt(v, f),
        }
    }
}

impl<Fp: Zero, Fq: Zero> Zero for FieldVariant<Fp, Fq>
where
    Self: Add<Self, Output = Self>,
{
    fn zero() -> Self {
        Self::Fp(Fp::zero())
    }

    fn is_zero(&self) -> bool {
        match self {
            Self::Fp(v) => v.is_zero(),
            Self::Fq(v) => v.is_zero(),
        }
    }
}

impl<Fp: One, Fq: One> One for FieldVariant<Fp, Fq>
where
    Self: Mul<Self, Output = Self>,
{
    fn one() -> Self {
        Self::Fp(Fp::one())
    }
}

impl<Fp: Add<Output = Fp>, Fq: Add<Output = Fq> + Add<Fp, Output = Fq>> Add<Self>
    for FieldVariant<Fp, Fq>
{
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Self::Fp(a), Self::Fp(b)) => Self::Fp(a + b),
            (Self::Fq(a), Self::Fq(b)) => Self::Fq(a + b),
            (Self::Fq(a), Self::Fp(b)) | (Self::Fp(b), Self::Fq(a)) => Self::Fq(a + b),
        }
    }
}

impl<Fp: Mul<Output = Fp>, Fq: Mul<Output = Fq> + Mul<Fp, Output = Fq>> Mul<Self>
    for FieldVariant<Fp, Fq>
{
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Self::Fp(a), Self::Fp(b)) => Self::Fp(a * b),
            (Self::Fq(a), Self::Fq(b)) => Self::Fq(a * b),
            (Self::Fq(a), Self::Fp(b)) | (Self::Fp(b), Self::Fq(a)) => Self::Fq(a * b),
        }
    }
}

impl<Fp: Neg<Output = Fp>, Fq: Neg<Output = Fq>> Neg for FieldVariant<Fp, Fq> {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self::Output {
        map!(self, neg)
    }
}

impl<Fp: Field, Fq: Field + Mul<Fp, Output = Fq>> Div<Self> for FieldVariant<Fp, Fq> {
    type Output = Self;

    #[inline]
    #[allow(clippy::suspicious_arithmetic_impl)]
    fn div(self, rhs: Self) -> Self::Output {
        self * rhs.inverse().unwrap()
    }
}

impl<Fp: Field, Fq: Field> Pow<usize> for FieldVariant<Fp, Fq> {
    type Output = Self;

    #[inline]
    fn pow(self, rhs: usize) -> Self::Output {
        map!(self, pow, &[rhs as u64, 0, 0, 0])
    }
}

impl<Fp: CanonicalSerialize, Fq: CanonicalSerialize> CanonicalSerialize for FieldVariant<Fp, Fq> {
    fn serialize_with_mode<W: ark_serialize::Write>(
        &self,
        mut writer: W,
        compress: ark_serialize::Compress,
    ) -> Result<(), ark_serialize::SerializationError> {
        match self {
            Self::Fp(v) => {
                FieldType::Fp.serialize_with_mode(&mut writer, compress)?;
                v.serialize_with_mode(writer, compress)
            }
            Self::Fq(v) => {
                FieldType::Fq.serialize_with_mode(&mut writer, compress)?;
                v.serialize_with_mode(writer, compress)
            }
        }
    }

    fn serialized_size(&self, compress: ark_serialize::Compress) -> usize {
        match self {
            Self::Fp(v) => FieldType::Fp.serialized_size(compress) + v.serialized_size(compress),
            Self::Fq(v) => FieldType::Fq.serialized_size(compress) + v.serialized_size(compress),
        }
    }
}

impl<Fp: Valid, Fq: Valid> Valid for FieldVariant<Fp, Fq> {
    fn check(&self) -> Result<(), ark_serialize::SerializationError> {
        match self {
            Self::Fp(v) => v.check(),
            Self::Fq(v) => v.check(),
        }
    }
}

impl<Fp: CanonicalDeserialize, Fq: CanonicalDeserialize> CanonicalDeserialize
    for FieldVariant<Fp, Fq>
{
    fn deserialize_with_mode<R: ark_serialize::Read>(
        mut reader: R,
        compress: ark_serialize::Compress,
        validate: ark_serialize::Validate,
    ) -> Result<Self, ark_serialize::SerializationError> {
        let field_type = FieldType::deserialize_with_mode(&mut reader, compress, validate)?;
        Ok(match field_type {
            FieldType::Fp => Self::Fp(Fp::deserialize_with_mode(reader, compress, validate)?),
            FieldType::Fq => Self::Fq(Fq::deserialize_with_mode(reader, compress, validate)?),
        })
    }
}

/// Shared vec between GPU and CPU.
/// Requirement is that the vec's memory is page aligned.
pub type GpuVec<T> = Vec<T, GpuAllocator>;

/// Allocator with page aligned allocations on Apple Silicon.
/// Uses global allocator on all other platforms.
pub struct GpuAllocator;

unsafe impl Allocator for GpuAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        #[cfg(apple_silicon)]
        return page_aligned_allocator::PageAlignedAllocator.allocate(layout);
        #[cfg(not(apple_silicon))]
        return ark_std::alloc::Global.allocate(layout);
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        #[cfg(apple_silicon)]
        return page_aligned_allocator::PageAlignedAllocator.deallocate(ptr, layout);
        #[cfg(not(apple_silicon))]
        return ark_std::alloc::Global.deallocate(ptr, layout);
    }
}

pub fn gpu_vec_to_vec<T>(v: GpuVec<T>) -> Vec<T> {
    let (ptr, length, capacity) = v.into_raw_parts();
    unsafe { Vec::from_raw_parts(ptr, length, capacity) }
}

pub fn vec_to_gpu_vec<T>(v: Vec<T>) -> GpuVec<T> {
    let (ptr, length, capacity) = v.into_raw_parts();
    unsafe { Vec::from_raw_parts_in(ptr, length, capacity, GpuAllocator) }
}

#[cfg(apple_silicon)]
mod page_aligned_allocator {
    use alloc::alloc::Global;
    use core::alloc::AllocError;
    use core::alloc::Allocator;
    use core::alloc::Layout;
    use core::ptr::NonNull;

    const PAGE_SIZE: usize = 16384;

    pub struct PageAlignedAllocator;

    unsafe impl Allocator for PageAlignedAllocator {
        fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
            Global.allocate(layout.align_to(PAGE_SIZE).unwrap().pad_to_align())
        }

        unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
            Global.deallocate(ptr, layout.align_to(PAGE_SIZE).unwrap().pad_to_align());
        }
    }
}

pub mod tests {
    use super::GpuAllocator;
    use crate::Matrix;
    use alloc::vec::Vec;
    use ark_ff::Field;
    use rand::Rng;

    /// Generates a matrix of fibbonacci sequence across two columns i.e.
    /// ┌───────┬───────┐
    /// │ Col 0 | Col 1 │
    /// ├───────┼───────┤
    /// │ 1     │ 1     │ #1 -> #2 ->
    /// ├───────┼───────┤
    /// │ 2     │ 3     │ #3 -> #4 ->
    /// ├───────┼───────┤
    /// │ 5     │ 8     │ #5 -> #6 ->
    /// ├───────┼───────┤
    /// │ ...   │ ...   │ ...
    /// └───────┴───────┘
    pub fn gen_fib_matrix<F: Field>(n: usize) -> Matrix<F> {
        let mut columns = vec![
            Vec::with_capacity_in(n, GpuAllocator),
            Vec::with_capacity_in(n, GpuAllocator),
        ];
        columns[0].push(F::one());
        columns[1].push(F::one());
        for _ in 1..n {
            let n0 = *columns[0].last().unwrap() + columns[1].last().unwrap();
            let n1 = n0 + columns[1].last().unwrap();
            columns[0].push(n0);
            columns[1].push(n1);
        }
        Matrix::new(columns)
    }

    /// Generates a single column matrix consisting of two values i.e.
    /// ┌───────┐
    /// │ Col 0 │
    /// ├───────┤
    /// │ 3     │
    /// ├───────┤
    /// │ 7     │
    /// ├───────┤
    /// │ 3     │
    /// ├───────┤
    /// │ 3     │
    /// ├───────┤
    /// │ 7     │
    /// ├───────┤
    /// │ ...   │
    /// └───────┘
    pub fn gen_binary_valued_matrix<F: Field>(n: usize, v1: F, v2: F) -> Matrix<F> {
        let mut rng = ark_std::test_rng();
        let mut col = Vec::with_capacity_in(n, GpuAllocator);
        col.resize_with(n, || if rng.gen() { v1 } else { v2 });
        Matrix::new(vec![col])
    }
}
