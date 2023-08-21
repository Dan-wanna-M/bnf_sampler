use std::ops::{IndexMut, Index, RangeTo};

#[derive(Clone, Debug)]
pub(crate) struct StackArena<T: Clone + Copy> {
    arena: Vec<Option<T>>,
    current_ptr: usize,
}

impl<T: Clone + Copy> StackArena<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        let mut area = Vec::with_capacity(capacity);
        area.resize(capacity, None);
        StackArena {
            arena: area,
            current_ptr: 0,
        }
    }

    pub fn allocate_a_stack(&mut self, capacity: usize) -> Stack<T> {
        let buffer = &mut self.arena[self.current_ptr..self.current_ptr + capacity];
        self.current_ptr += capacity;
        Stack { buffer, top: 0 }
    }

    pub fn clear(&mut self) {
        for i in 0..self.current_ptr {
            self.arena[i] = None;
        }
        self.current_ptr = 0;
    }
}
#[derive(Debug)]
pub(crate) struct Stack<'a, T: Copy> {
    buffer: &'a mut [Option<T>],
    top: usize,
}

impl<'a, T: Copy> Index<usize> for Stack<'a, T>  {
    type Output=T;

    fn index(&self, index: usize) -> &Self::Output {
        assert!(index<self.top, "the length of the stack is {}, but the index is {}", self.top, index);
        self.buffer[index].as_ref().unwrap()
    }
}
impl<'a, T: Copy> Index<RangeTo<usize>> for Stack<'a, T>  {
    type Output=[Option<T>];

    fn index(&self, index: RangeTo<usize>) -> &Self::Output {
        assert!(index.end<self.top, "the length of the stack is {}, but the range is {:?}", self.top, index);
        &self.buffer[index]
    }
}

impl<'a, T: Copy> Stack<'a, T> {
    pub fn push(&mut self, value: T) {
        self.buffer[self.top] = Some(value);
        self.top += 1;
    }
    pub fn pop(&mut self) -> Option<T> {
        if self.top == 0 {
            return None;
        }
        let result = self.buffer[self.top - 1]
            .unwrap_or_else(|| panic!("The popped value should be valid."));
        self.buffer[self.top - 1] = None;
        self.top -= 1;
        Some(result)
    }
    pub fn last(&self) -> Option<T> {
        if self.top == 0 {
            return None;
        }
        let result = self.buffer[self.top - 1]
            .unwrap_or_else(|| panic!("The popped value should be valid."));
        Some(result)
    }

    pub fn copy_from_slice(&mut self, source: &[T]) {
        assert!(self.top == 0);
        assert!(self.buffer.len() >= source.len());
        for (i, value) in source.iter().enumerate() {
            self.buffer[i] = Some(*value);
        }
        self.top = source.len();
    }
    pub fn copy_from_raw_slice(&mut self, source: &[Option<T>]) {
        assert!(self.top == 0);
        assert!(self.buffer.len() >= source.len());
        for (i, value) in source.iter().enumerate() {
            self.buffer[i] = *value;
        }
        self.top = source.len();
    }

    pub fn as_raw_slice(&self)->&[Option<T>]
    {
        &self.buffer[..self.top]
    }

    pub fn to_vec(&self) -> Vec<T> {
        let mut temp = Vec::with_capacity(self.top);
        for i in 0..self.top {
            temp.push(
                self.buffer[i].unwrap_or_else(|| panic!("The popped value should be valid.")),
            );
        }
        temp
    }

    pub fn len(&self) -> usize {
        self.top
    }

    pub fn copy_from(&mut self, source: &Self) {
        assert!(self.top == 0);
        for i in 0..source.top {
            self.buffer[i] = source.buffer[i];
        }
        self.top = source.top;
    }
}
