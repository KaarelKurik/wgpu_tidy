// NB!
// I cannot guarantee the correctness of this implementation.
// Slang's reflection logic and its corresponding documentation
// seems to still be in a very raw state at time of writing.
// Issues like `getBindingSpace` being deprecated, and navigating
// from one descriptor set to another using the cursor method,
// are not addressed in the documentation.
// Naive recursive implementations of offset calculations do not work
// due to strange quirks in where and how offsets are assigned
// for descriptor sets.
// Thus this code was written by inference, experimentation and guesswork,
// rather than made to conform with a clear spec. Likely wrong at edge cases.
// Use at own risk.

use std::{collections::HashMap, task::Wake};

use bytemuck::Contiguous;
use slang::{
    reflection::{TypeLayout, VariableLayout},
    TypeKind,
};
use wgpu::{
    BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry, ShaderStages,
    TextureViewDimension,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct Offset {
    set: usize,
    set_accumulator: usize,
    slot: usize,
    slot_accumulator: usize,
    uniform: usize,
}

#[derive(Clone, Copy)]
pub struct Cursor<'a> {
    type_layout: &'a TypeLayout,
    offset: Offset,
}

impl<'a> Cursor<'a> {
    pub fn fresh(type_layout: &'a TypeLayout) -> Cursor<'a> {
        Cursor {
            type_layout,
            offset: Offset::default(),
        }
    }

    fn navigate_into_var(&self, vl: &'a VariableLayout) -> Option<Cursor<'a>> {
        let tl = vl.type_layout();
        let set_accumulator = self.offset.set_accumulator
            + vl.offset(slang::ParameterCategory::SubElementRegisterSpace);
        // The actual kind condition is [anything that can introduce its own set], I think
        // Maybe wrong on that
        let set = if tl.kind() == TypeKind::ParameterBlock {
            set_accumulator
        } else {
            self.offset.set
        };
        let slot_accumulator = if set != self.offset.set {
            0
        } else {
            self.offset.slot_accumulator + vl.offset(slang::ParameterCategory::DescriptorTableSlot)
        };
        let slot = if set != self.offset.set {
            0
        } else {
            // The actual kind condition is [anything that can introduce its own slot], I think
            match tl.kind() {
                TypeKind::ConstantBuffer
                | TypeKind::Resource
                | TypeKind::SamplerState
                | TypeKind::TextureBuffer
                | TypeKind::ShaderStorageBuffer
                | TypeKind::ParameterBlock => slot_accumulator,
                _ => self.offset.slot,
            }
        };
        let uniform = if set != self.offset.set || slot != self.offset.slot {
            0
        } else {
            self.offset.uniform + vl.offset(slang::ParameterCategory::Uniform)
        };
        Some(Cursor {
            type_layout: tl,
            offset: Offset {
                set,
                set_accumulator,
                slot,
                slot_accumulator,
                uniform,
            },
        })
    }
    fn navigate_field(&self, field_index: u32) -> Option<Cursor<'a>> {
        let field_var = self.type_layout.field_by_index(field_index)?;
        self.navigate_into_var(field_var)
    }
    fn navigate_child(&self) -> Option<Cursor<'a>> {
        match self.type_layout.kind() {
            TypeKind::ConstantBuffer
            | TypeKind::TextureBuffer
            | TypeKind::ShaderStorageBuffer
            | TypeKind::ParameterBlock => {
                let child_var = self.type_layout.element_var_layout();
                self.navigate_into_var(child_var)
            }
            _ => None,
        }
    }
}

pub fn walk_him_down(vl: &VariableLayout, c: Cursor) {
    println!(
        "{}:{};{:?}",
        vl.variable().name().unwrap_or("<anon_var>"),
        vl.type_layout().name().unwrap_or("<anon_type>"),
        vl.type_layout().kind()
    );
    println!("{:?}", c.offset);
    let field_count = vl.type_layout().field_count();
    for i in 0..field_count {
        let fc = c.navigate_field(i).unwrap();
        walk_him_down(vl.type_layout().field_by_index(i).unwrap(), fc);
    }
    if let Some(ec) = c.navigate_child() {
        walk_him_down(vl.type_layout().element_var_layout(), ec);
    }
}
