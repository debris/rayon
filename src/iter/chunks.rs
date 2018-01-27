use std::cmp::min;

use ::math::div_round_up;
use super::plumbing::*;
use super::*;

#[must_use = "iterator adaptors are lazy and do nothing unless consumed"]
#[derive(Debug, Clone)]
pub struct Chunks<I>
    where I: IndexedParallelIterator
{
    size: usize,
    i: I,
}

/// Create a new `Chunks` iterator
///
/// NB: a free fn because it is NOT part of the end-user API.
pub fn new<I>(i: I, size: usize) -> Chunks<I>
    where I: IndexedParallelIterator
{
    Chunks { i: i, size: size }
}

impl<I> ParallelIterator for Chunks<I>
    where I: IndexedParallelIterator
{
    type Item = Vec<I::Item>;

    fn drive_unindexed<C>(self, consumer: C) -> C::Result
        where C: Consumer<Vec<I::Item>>
    {
        bridge(self, consumer)
    }

    fn opt_len(&self) -> Option<usize> {
        Some(self.len())
    }
}

impl<I> IndexedParallelIterator for Chunks<I>
    where I: IndexedParallelIterator
{
    fn drive<C>(self, consumer: C) -> C::Result
        where C: Consumer<Self::Item>
    {
        bridge(self, consumer)
    }

    fn len(&self) -> usize {
        div_round_up(self.i.len(), self.size)
    }

    fn with_producer<CB>(self, callback: CB) -> CB::Output
        where CB: ProducerCallback<Self::Item>
    {
        let len = self.i.len();
        return self.i.with_producer(Callback {
            size: self.size,
            len: len,
            callback: callback,
        });

        struct Callback<CB> {
            size: usize,
            len: usize,
            callback: CB,
        }

        impl<T, CB> ProducerCallback<T> for Callback<CB>
            where CB: ProducerCallback<Vec<T>>
        {
            type Output = CB::Output;

            fn callback<P>(self, base: P) -> CB::Output
                where P: Producer<Item = T>
            {
                self.callback.callback(ChunkProducer {
                    chunk_size: self.size,
                    len: self.len,
                    base: base,
                })
            }
        }
    }
}

struct ChunkProducer<P>
    where P: Producer
{
    chunk_size: usize,
    len: usize,
    base: P,
}

impl<P> Producer for ChunkProducer<P>
    where P: Producer
{
    type Item = Vec<P::Item>;
    type IntoIter = ChunkSeq<P>;

    fn into_iter(self) -> Self::IntoIter {
        ChunkSeq {
            chunk_size: self.chunk_size,
            len: self.len,
            inner: if self.len > 0 {
                Some(self.base)
            } else {
                None
            }
        }
    }

    fn split_at(self, index: usize) -> (Self, Self) {
        let elem_index = min(index * self.chunk_size, self.len);
        let (left, right) = self.base.split_at(elem_index);
        (ChunkProducer {
            chunk_size: self.chunk_size,
            len: elem_index,
            base: left,
        },
        ChunkProducer {
            chunk_size: self.chunk_size,
            len: self.len - elem_index,
            base: right,
        })
    }

    fn min_len(&self) -> usize {
        div_round_up(self.base.min_len(), self.chunk_size)
    }

    fn max_len(&self) -> usize {
        self.base.max_len() / self.chunk_size
    }
}

struct ChunkSeq<P> {
    chunk_size: usize,
    len: usize,
    inner: Option<P>,
}

impl<P> Iterator for ChunkSeq<P>
    where P: Producer
{
    type Item = Vec<P::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.inner.take() {
            Some(producer) => if self.len > self.chunk_size {
                let (left, right) = producer.split_at(self.chunk_size);
                self.inner = Some(right);
                self.len -= self.chunk_size;
                Some(left.into_iter().collect())
            } else {
                debug_assert!(self.len > 0);
                self.len = 0;
                Some(producer.into_iter().collect())
            },
            _ => None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl<P> ExactSizeIterator for ChunkSeq<P>
    where P: Producer
{
    #[inline]
    fn len(&self) -> usize {
        div_round_up(self.len, self.chunk_size)
    }
}

impl<P> DoubleEndedIterator for ChunkSeq<P>
    where P: Producer
{
    fn next_back(&mut self) -> Option<Self::Item> {
        match self.inner.take() {
            Some(producer) => if self.len > self.chunk_size {
                let mut size = self.len % self.chunk_size;
                if size == 0 {
                    size = self.chunk_size;
                }
                let (left, right) = producer.split_at(self.len - size);
                self.inner = Some(left);
                self.len -= size;
                Some(right.into_iter().collect())
            } else {
                debug_assert!(self.len > 0);
                self.len = 0;
                Some(producer.into_iter().collect())
            },
            _ => None
        }
    }
}
