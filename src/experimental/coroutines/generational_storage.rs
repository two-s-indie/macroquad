#[derive(Clone, Copy, Debug)]
pub struct GenerationalId {
    id: usize,
    generation: usize,
}

#[cfg(test)]
mod tests {
    use super::GenerationalStorage;

    #[test]
    fn freeing_a_handle_twice_does_not_duplicate_a_free_slot() {
        let mut storage = GenerationalStorage::new();
        let old = storage.push(1);
        let _ = storage.take(old);
        let _ = storage.take(old);

        let first = storage.push(2);
        let second = storage.push(3);
        assert_eq!(storage.get(first), Some(&2));
        assert_eq!(storage.get(second), Some(&3));
        assert_eq!(storage.count(), 2);
    }

    #[test]
    fn cleared_handles_can_be_queried_and_freed() {
        let mut storage = GenerationalStorage::new();
        let old = storage.push(1);
        drop(storage.take_all());

        assert!(storage.get(old).is_none());
        assert!(storage.get_mut(old).is_none());
        let _ = storage.take(old);
        assert_eq!(storage.count(), 0);
    }

    #[test]
    fn clearing_does_not_revive_old_handles_when_slots_are_reused() {
        let mut storage = GenerationalStorage::new();
        let old = [storage.push(1), storage.push(2), storage.push(3)];
        let _ = storage.take(old[1]);
        drop(storage.take_all());
        drop(storage.take_all());

        let current = [storage.push(4), storage.push(5), storage.push(6)];
        for id in old {
            assert!(storage.get(id).is_none());
            assert!(storage.get_mut(id).is_none());
            let _ = storage.take(id);
        }
        for (id, value) in current.into_iter().zip(4..=6) {
            assert_eq!(storage.get(id), Some(&value));
        }
        assert_eq!(storage.count(), 3);
    }

    #[test]
    fn freeing_a_completed_handle_does_not_duplicate_a_free_slot() {
        let mut storage = GenerationalStorage::new();
        let completed = storage.push(1);
        let _ = storage.take(completed);
        let _ = storage.take(completed);

        let first = storage.push(2);
        let second = storage.push(3);
        let _ = storage.take(completed);
        assert_eq!(storage.get(first), Some(&2));
        assert_eq!(storage.get(second), Some(&3));
    }
}

struct GenerationalCell<T> {
    generation: usize,
    state: T,
}

pub(crate) struct GenerationalStorage<T> {
    vec: Vec<Option<GenerationalCell<T>>>,
    free_indices: Vec<(usize, usize)>,
}

impl<T> GenerationalStorage<T> {
    pub fn new() -> GenerationalStorage<T> {
        GenerationalStorage {
            vec: Vec::with_capacity(1000),
            free_indices: Vec::with_capacity(100),
        }
    }

    pub fn push(&mut self, data: T) -> GenerationalId {
        let generation;

        let id = if let Some((free_id, old_generation)) = self.free_indices.pop() {
            assert!(self.vec[free_id].is_none());

            generation = old_generation + 1;
            self.vec[free_id] = Some(GenerationalCell {
                state: data,
                generation,
            });
            free_id
        } else {
            generation = 0;
            self.vec.push(Some(GenerationalCell {
                state: data,
                generation,
            }));
            self.vec.len() - 1
        };

        GenerationalId { id, generation }
    }

    pub fn get(&self, id: GenerationalId) -> Option<&T> {
        let cell = self.vec.get(id.id)?.as_ref()?;
        if cell.generation != id.generation {
            return None;
        }

        Some(&cell.state)
    }

    pub fn get_mut(&mut self, id: GenerationalId) -> Option<&mut T> {
        let cell = self.vec.get_mut(id.id)?.as_mut()?;
        if cell.generation != id.generation {
            return None;
        }

        Some(&mut cell.state)
    }

    pub fn count(&self) -> usize {
        self.vec.iter().filter(|c| c.is_some()).count()
    }

    pub fn ids(&self) -> Vec<GenerationalId> {
        self.vec
            .iter()
            .enumerate()
            .filter_map(|(id, cell)| {
                cell.as_ref().map(|cell| GenerationalId {
                    id,
                    generation: cell.generation,
                })
            })
            .collect()
    }

    /// Detach values without running their destructors under the caller's borrow.
    pub fn take_all(&mut self) -> Vec<T> {
        let mut removed = Vec::new();
        for (id, slot) in self.vec.iter_mut().enumerate() {
            if let Some(cell) = slot.take() {
                self.free_indices.push((id, cell.generation));
                removed.push(cell.state);
            }
        }
        removed
    }

    pub fn take(&mut self, id: GenerationalId) -> Option<T> {
        // Finished, stopped, and outdated handles must all be harmless to free.
        if self.get(id).is_none() {
            return None;
        }

        self.free_indices.push((id.id, id.generation));
        self.vec[id.id].take().map(|cell| cell.state)
    }

    pub(crate) fn allocated_memory(&self) -> usize {
        self.vec.capacity() * size_of::<GenerationalCell<T>>()
    }
}
