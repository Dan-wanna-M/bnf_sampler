#[derive(Clone, Debug)]
pub struct StackArena<T: Clone + Copy> {
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
pub struct Stack<'a, T: Copy> {
    buffer: &'a mut [Option<T>],
    top: usize,
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
        assert!(self.top==0);
        assert!(self.buffer.len() >= source.len());
        for i in 0..source.len() {
            self.buffer[i] = Some(source[i]);
        }
        self.top = source.len();
    }

    pub fn to_vec(&self)->Vec<T>
    {
        let mut temp = Vec::with_capacity(self.top);
        for i in 0..self.top
        {
            temp.push(self.buffer[i].unwrap_or_else(||panic!("The popped value should be valid.")));
        }
        temp
    }

    pub fn len(&self)->usize
    {
        self.top
    }

    pub fn copy_from(&mut self, source: &Self)
    {
        assert!(self.top==0);
        for i in 0..source.top
        {
            self.buffer[i] = source.buffer[i];
        }
        self.top = source.top;
    }
}
