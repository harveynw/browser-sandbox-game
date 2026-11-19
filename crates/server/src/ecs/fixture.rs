//! Test-only convenience access to the authoritative Bevy world.
use super::*;

use protocol::EntityId;

/// Wraps the ECS world for owned scenario fixtures.
pub struct EntityStore {
    /// ECS world containing the fixture's actor state.
    pub(super) world: Box<World>,
}

impl Default for EntityStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EntityStore {
    /// Creates an indexed fixture world and registers its gameplay message queues.
    pub fn new() -> Self {
        let mut world = World::new();
        world.init_resource::<EntityIndex>();
        crate::ecs::messages::register(&mut world);
        Self {
            world: Box::new(world),
        }
    }
    /// Transfers the fixture's ECS world to its caller.
    pub(crate) fn into_world(mut self) -> World {
        crate::component::tests::install_fixture_actors(&mut self.world);
        *self.world
    }
    /// Wraps an existing ECS world for fixture access.
    #[cfg(test)]
    pub(crate) fn from_world(world: World) -> Self {
        Self {
            world: Box::new(world),
        }
    }
}

impl EntityStore {
    /// Test fixtures inject facts into the same Bevy messages as native systems.
    #[cfg(test)]
    pub(crate) fn write_message<M: Message>(&mut self, message: M) {
        self.world
            .write_message(message)
            .expect("gameplay message type registered");
    }
    /// Creates an indexed fixture actor with its stable gameplay ID.
    pub fn spawn(&mut self, id: EntityId, components: impl Bundle) -> Result<Entity, String> {
        if self.world.resource::<EntityIndex>().0.contains_key(&id.0) {
            return Err(format!("duplicate game entity ID {}", id.0));
        }
        let entity = self.world.spawn(components).insert(GameId(id)).id();
        self.world
            .resource_mut::<EntityIndex>()
            .0
            .insert(id.0, entity);
        Ok(entity)
    }
    /// Finds the current ECS entity for a stable gameplay ID.
    pub fn entity(&self, id: EntityId) -> Option<Entity> {
        self.world.resource::<EntityIndex>().0.get(&id.0).copied()
    }
    pub fn get<C: Component>(&self, id: EntityId) -> Option<&C> {
        self.world.get::<C>(self.entity(id)?)
    }
    pub fn get_mut<C: Component<Mutability = bevy_ecs::component::Mutable>>(
        &mut self,
        id: EntityId,
    ) -> Option<&mut C> {
        let entity = self.entity(id)?;
        self.world.get_mut::<C>(entity).map(Mut::into_inner)
    }
    /// Finds a mutable component through the stable gameplay-ID index.
    pub fn get_mut_by_id<C: Component<Mutability = bevy_ecs::component::Mutable>>(
        &mut self,
        id: &u32,
    ) -> Option<&mut C> {
        self.get_mut::<C>(EntityId(*id))
    }
    /// Adds or replaces an actor component without changing other actor state.
    pub fn insert<T: Component>(&mut self, id: u32, value: T) -> Option<T> {
        let entity = if let Some(entity) = self.entity(EntityId(id)) {
            entity
        } else {
            self.spawn(EntityId(id), ()).expect("fresh game ID")
        };
        let old = self.world.entity_mut(entity).take::<T>();
        self.world.entity_mut(entity).insert(value);
        old
    }
    /// Removes the requested component while preserving the actor's other components.
    pub fn remove<T: Component>(&mut self, id: &u32) -> Option<T> {
        let entity = self.entity(EntityId(*id))?;
        let old = self.world.entity_mut(entity).take::<T>();
        if old.is_some() && self.world.entity(entity).archetype().components().len() == 1 {
            self.despawn(EntityId(*id));
        }
        old
    }
    /// Builds a borrowed view of the state exposed to its caller.
    pub fn view<T: Component>(&self) -> ComponentView<'_, T> {
        ComponentView {
            world: &self.world,
            marker: PhantomData,
        }
    }
    /// Visits mutable components in stable gameplay-ID order.
    pub fn values_mut<T: Component<Mutability = bevy_ecs::component::Mutable>>(
        &mut self,
    ) -> impl Iterator<Item = &mut T> {
        let mut query = self.world.query::<(&GameId, &mut T)>();
        let mut items: Vec<_> = query
            .iter_mut(&mut self.world)
            .map(|(id, value)| (id.0.0, value.into_inner()))
            .collect();
        items.sort_by_key(|(id, _)| *id);
        items.into_iter().map(|(_, value)| value)
    }
    /// Clears transient state after its current processing batch.
    pub fn clear<T: Component>(&mut self) {
        let ids: Vec<_> = self.view::<T>().keys().copied().collect();
        for id in ids {
            self.remove::<T>(&id);
        }
    }
    /// Removes components that fail the supplied predicate.
    pub fn retain<T: Component<Mutability = bevy_ecs::component::Mutable>>(
        &mut self,
        mut keep: impl FnMut(&u32, &mut T) -> bool,
    ) {
        let ids: Vec<_> = self.view::<T>().keys().copied().collect();
        for id in ids {
            if !keep(
                &id,
                self.get_mut_by_id::<T>(&id).expect("indexed component"),
            ) {
                self.remove::<T>(&id);
            }
        }
    }
    pub fn values<C: crate::ecs::values::ComponentValue>(&self) -> ValueView<'_, C> {
        ValueView {
            inner: self.view::<C>(),
        }
    }
    /// Returns mutable access to the wrapped component value.
    pub fn value_mut<C: crate::ecs::values::ComponentValue>(
        &mut self,
        id: &u32,
    ) -> Option<&mut C::Value> {
        self.get_mut_by_id::<C>(id).map(C::value_mut)
    }
    /// Adds a typed value component to the requested actor.
    pub fn insert_value<C: crate::ecs::values::ComponentValue>(
        &mut self,
        id: u32,
        value: C::Value,
    ) -> Option<C> {
        self.insert(id, C::new(value))
    }
    /// Returns the actor's value, creating its component if necessary.
    pub fn value_or_insert<C: crate::ecs::values::ComponentValue>(
        &mut self,
        id: u32,
        value: C::Value,
    ) -> &mut C::Value {
        if self.get::<C>(EntityId(id)).is_none() {
            self.insert(id, C::new(value));
        }
        self.value_mut::<C>(&id).expect("inserted metadata")
    }
    /// Removes value components that fail the supplied predicate.
    pub fn retain_value<C: crate::ecs::values::ComponentValue>(
        &mut self,
        mut keep: impl FnMut(&u32, &mut C::Value) -> bool,
    ) {
        self.retain::<C>(|id, component| keep(id, component.value_mut()));
    }
    /// Removes an actor and its stable gameplay-ID mapping.
    pub fn despawn(&mut self, id: EntityId) -> bool {
        let Some(entity) = self.world.resource_mut::<EntityIndex>().0.remove(&id.0) else {
            return false;
        };
        self.world.despawn(entity)
    }
    /// Snapshot handles in game-ID order, not archetype/storage order.
    pub fn ordered(&self) -> Vec<(EntityId, Entity)> {
        self.world
            .resource::<EntityIndex>()
            .0
            .iter()
            .map(|(&id, &entity)| (EntityId(id), entity))
            .collect()
    }
}

/// Read-only ordered component projection. This holds no entity data or second index.
pub struct ComponentView<'w, T> {
    /// ECS world containing the fixture's actor state.
    world: &'w World,
    /// Associates this view with its component type without owning a value.
    marker: PhantomData<T>,
}

impl<'w, T: Component> ComponentView<'w, T> {
    pub fn get(&self, id: &u32) -> Option<&'w T> {
        self.world
            .resource::<EntityIndex>()
            .0
            .get(id)
            .and_then(|e| self.world.get::<T>(*e))
    }
    pub fn contains_key(&self, id: &u32) -> bool {
        self.get(id).is_some()
    }
    pub fn iter(&self) -> impl Iterator<Item = (&'w u32, &'w T)> + 'w {
        let world = self.world;
        world
            .resource::<EntityIndex>()
            .0
            .iter()
            .filter_map(move |(id, entity)| world.get::<T>(*entity).map(|value| (id, value)))
    }
    pub fn values(&self) -> impl Iterator<Item = &'w T> + 'w {
        self.iter().map(|(_, value)| value)
    }
    pub fn keys(&self) -> impl Iterator<Item = &'w u32> + 'w {
        self.iter().map(|(id, _)| id)
    }
    pub fn len(&self) -> usize {
        self.iter().count()
    }
    pub fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }
}

impl<T: Component> Index<&u32> for ComponentView<'_, T> {
    type Output = T;
    fn index(&self, id: &u32) -> &T {
        self.get(id).expect("missing component")
    }
}

impl<'w, T: Component> IntoIterator for &ComponentView<'w, T> {
    type Item = (&'w u32, &'w T);
    type IntoIter = std::vec::IntoIter<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter().collect::<Vec<_>>().into_iter()
    }
}

impl<'w, T: Component> IntoIterator for ComponentView<'w, T> {
    type Item = (&'w u32, &'w T);
    type IntoIter = std::vec::IntoIter<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter().collect::<Vec<_>>().into_iter()
    }
}

/// A read-only payload projection onto named metadata components.
pub struct ValueView<'w, C: crate::ecs::values::ComponentValue> {
    /// Component view wrapped by this value adapter.
    inner: ComponentView<'w, C>,
}

impl<'w, C: crate::ecs::values::ComponentValue> ValueView<'w, C> {
    pub fn get(&self, id: &u32) -> Option<&'w C::Value> {
        self.inner.get(id).map(C::value)
    }
    pub fn contains_key(&self, id: &u32) -> bool {
        self.inner.contains_key(id)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&'w u32, &'w C::Value)> + 'w {
        self.inner.iter().map(|(id, c)| (id, c.value()))
    }
    pub fn values(&self) -> impl Iterator<Item = &'w C::Value> + 'w {
        self.iter().map(|(_, v)| v)
    }
    pub fn keys(&self) -> impl Iterator<Item = &'w u32> + 'w {
        self.inner.keys()
    }
    pub fn len(&self) -> usize {
        self.inner.len()
    }
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl<C: crate::ecs::values::ComponentValue> Index<&u32> for ValueView<'_, C> {
    type Output = C::Value;
    fn index(&self, id: &u32) -> &Self::Output {
        self.get(id).expect("missing entity metadata")
    }
}

impl EntityStore {
    pub(crate) fn live(&mut self) -> crate::component::tests::LiveActors<'_> {
        crate::component::tests::install_fixture_actors(&mut self.world);
        crate::component::tests::LiveActors(&mut self.world)
    }
}
