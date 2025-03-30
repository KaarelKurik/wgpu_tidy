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

use std::{collections::HashMap, num::NonZero, task::Wake};

use bytemuck::Contiguous;
use slang::{
    ParameterCategory, TypeKind,
    reflection::{TypeLayout, VariableLayout},
};
use wgpu::{
    BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry, Buffer,
    BufferBinding, Sampler, ShaderStages, Texture, TextureView, TextureViewDimension,
    util::DeviceExt,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct Offset {
    set: usize,
    set_accumulator: usize,
    slot: usize,
    slot_accumulator: usize,
    uniform: usize,
}

impl Offset {
    pub fn set(&self) -> usize {
        self.set
    }

    pub fn set_accumulator(&self) -> usize {
        self.set_accumulator
    }

    pub fn slot(&self) -> usize {
        self.slot
    }

    pub fn slot_accumulator(&self) -> usize {
        self.slot_accumulator
    }

    pub fn uniform(&self) -> usize {
        self.uniform
    }
}

pub trait Writable {
    fn write_at_cursor(
        &self,
        c: Cursor,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        binding_resources: &mut BindingResources,
    );
}

pub struct BindingResources {
    pub buffers: HashMap<usize, HashMap<usize, Buffer>>,
    pub texture_views: HashMap<usize, HashMap<usize, TextureView>>,
    pub textures: HashMap<usize, HashMap<usize, Texture>>,
    pub samplers: HashMap<usize, HashMap<usize, Sampler>>,
}

pub fn buffers_from_layout(
    device: &wgpu::Device,
    layout_entries: &HashMap<usize, Vec<BindGroupLayoutEntry>>,
) -> HashMap<usize, HashMap<usize, Buffer>> {
    let mut out = HashMap::new();
    for (&bind_group_index, v) in layout_entries {
        let mut cur_entry = HashMap::new();
        for le in v {
            if let wgpu::BindingType::Buffer {
                ty,
                has_dynamic_offset,
                min_binding_size,
            } = le.ty
            {
                debug_assert!(!has_dynamic_offset);
                let usage = wgpu::BufferUsages::COPY_DST
                    | match ty {
                        wgpu::BufferBindingType::Uniform => {
                            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_SRC
                        }
                        wgpu::BufferBindingType::Storage { read_only } => {
                            wgpu::BufferUsages::STORAGE
                                | if read_only {
                                    wgpu::BufferUsages::empty()
                                } else {
                                    wgpu::BufferUsages::COPY_SRC
                                }
                        }
                    };
                let size = min_binding_size.map_or(0u64, |x| x.into());
                cur_entry.insert(
                    le.binding as usize,
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: None,
                        size,
                        usage,
                        mapped_at_creation: false,
                    }),
                );
            }
        }
        out.insert(bind_group_index, cur_entry);
    }
    out
}

// No analogue for textures, we'll make them dynamically

// Dynamic buffers still need to be resized after
pub fn bind_group_entries_from_layout<'a>(
    layout_entries: &HashMap<usize, Vec<BindGroupLayoutEntry>>,
    binding_resources: &'a BindingResources,
) -> HashMap<usize, Vec<BindGroupEntry<'a>>> {
    layout_entries
        .iter()
        .map(|(k, v)| {
            (
                *k,
                v.iter()
                    .map(|le| {
                        let resource = match le.ty {
                            wgpu::BindingType::Buffer {
                                ty,
                                has_dynamic_offset,
                                min_binding_size,
                            } => {
                                let buffer = binding_resources
                                    .buffers
                                    .get(k)
                                    .unwrap()
                                    .get(&le.binding.try_into().unwrap())
                                    .unwrap();
                                wgpu::BindingResource::Buffer(BufferBinding {
                                    buffer,
                                    offset: 0,
                                    size: None,
                                })
                            }
                            wgpu::BindingType::Texture {
                                sample_type,
                                view_dimension,
                                multisampled,
                            } => {
                                let texture_view = binding_resources
                                    .texture_views
                                    .get(k)
                                    .unwrap()
                                    .get(&le.binding.try_into().unwrap())
                                    .unwrap();
                                wgpu::BindingResource::TextureView(texture_view)
                            }
                            wgpu::BindingType::Sampler(sbt) => {
                                let sampler = binding_resources
                                    .samplers
                                    .get(k)
                                    .unwrap()
                                    .get(&le.binding.try_into().unwrap())
                                    .unwrap();
                                wgpu::BindingResource::Sampler(sampler)
                            }
                            _ => unimplemented!(),
                        };
                        BindGroupEntry {
                            binding: le.binding,
                            resource,
                        }
                    })
                    .collect(),
            )
        })
        .collect()
}

pub fn base_layout_entries(tl: &TypeLayout) -> HashMap<usize, Vec<BindGroupLayoutEntry>> {
    let mut entries = HashMap::new();
    layout_entries_wowee(tl, &mut entries, 0, 0);
    entries
}

// Assumes no explicit binding!
pub fn layout_entries_wowee(
    tl: &TypeLayout,
    entries: &mut HashMap<usize, Vec<BindGroupLayoutEntry>>,
    current_set_index: usize,
    next_set_index: usize,
) {
    let binding_range_count = tl.binding_range_count();
    let mut next_set_index = next_set_index;
    for i in 0..binding_range_count {
        let leaf_tl = tl.binding_range_leaf_type_layout(i);
        match tl.binding_range_type(i) {
            slang::BindingType::Sampler => {
                if !entries.contains_key(&current_set_index) {
                    entries.insert(current_set_index, vec![]);
                    next_set_index += 1;
                }
                let entry_vec = entries.get_mut(&current_set_index).unwrap();
                let binding: u32 = (entry_vec.len()).try_into().unwrap();
                entry_vec.push(BindGroupLayoutEntry {
                    binding,
                    visibility: ShaderStages::all(),
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                });
            }
            slang::BindingType::Texture => {
                if !entries.contains_key(&current_set_index) {
                    entries.insert(current_set_index, vec![]);
                    next_set_index += 1;
                }
                let entry_vec = entries.get_mut(&current_set_index).unwrap();
                let binding = (entry_vec.len()).try_into().unwrap();
                let leaf_ty = leaf_tl.ty().unwrap();
                let view_dimension = match leaf_ty.resource_shape() {
                    slang::ResourceShape::SlangTexture1d => TextureViewDimension::D1,
                    slang::ResourceShape::SlangTexture2d => TextureViewDimension::D2,
                    slang::ResourceShape::SlangTexture3d => TextureViewDimension::D3,
                    slang::ResourceShape::SlangTextureCube => TextureViewDimension::Cube,
                    slang::ResourceShape::SlangTextureCubeArray => TextureViewDimension::CubeArray,
                    slang::ResourceShape::SlangTexture2dArray => TextureViewDimension::D2Array,
                    _ => {
                        unreachable!()
                    }
                };
                entry_vec.push(BindGroupLayoutEntry {
                    binding,
                    visibility: ShaderStages::all(),
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension,
                        multisampled: false,
                    },
                    count: None,
                });
            }
            slang::BindingType::ConstantBuffer => {
                if leaf_tl
                    .container_var_layout()
                    .type_layout()
                    .size(slang::ParameterCategory::DescriptorTableSlot)
                    > 0
                {
                    let unif_size = leaf_tl
                        .element_var_layout()
                        .type_layout()
                        .size(slang::ParameterCategory::Uniform);
                    if !entries.contains_key(&current_set_index) {
                        entries.insert(current_set_index, vec![]);
                        next_set_index += 1;
                    }
                    let entry_vec = entries.get_mut(&current_set_index).unwrap();
                    let binding: u32 = entry_vec.len().try_into().unwrap();
                    entry_vec.push(BindGroupLayoutEntry {
                        binding,
                        visibility: ShaderStages::all(),
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZero::new(unif_size.try_into().unwrap()),
                        },
                        count: None,
                    });
                }
                layout_entries_wowee(
                    leaf_tl.element_type_layout(),
                    entries,
                    current_set_index,
                    next_set_index,
                );
            }
            slang::BindingType::ParameterBlock => {
                if leaf_tl
                    .container_var_layout()
                    .type_layout()
                    .size(slang::ParameterCategory::SubElementRegisterSpace)
                    > 0
                {
                    let current_set_index = next_set_index;
                    entries.insert(current_set_index, vec![]);
                    next_set_index += 1;
                }
                if leaf_tl
                    .container_var_layout()
                    .type_layout()
                    .size(slang::ParameterCategory::DescriptorTableSlot)
                    > 0
                {
                    let unif_size = leaf_tl
                        .element_var_layout()
                        .type_layout()
                        .size(slang::ParameterCategory::Uniform);
                    let entry_vec = entries.get_mut(&current_set_index).unwrap();
                    let binding: u32 = entry_vec.len().try_into().unwrap();
                    entry_vec.push(BindGroupLayoutEntry {
                        binding,
                        visibility: ShaderStages::all(),
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZero::new(unif_size.try_into().unwrap()),
                        },
                        count: None,
                    });
                }
                layout_entries_wowee(
                    leaf_tl.element_type_layout(),
                    entries,
                    current_set_index,
                    next_set_index,
                );
            }
            slang::BindingType::RawBuffer | slang::BindingType::TypedBuffer => {
                if !entries.contains_key(&current_set_index) {
                    entries.insert(current_set_index, vec![]);
                    next_set_index += 1;
                }
                let entry_vec = entries.get_mut(&current_set_index).unwrap();
                let binding = (entry_vec.len()).try_into().unwrap();
                let min_binding_size: NonZero<u64> = NonZero::new(
                    leaf_tl
                        .element_type_layout()
                        .size(slang::ParameterCategory::Uniform)
                        .try_into()
                        .unwrap(),
                )
                .unwrap();
                entry_vec.push(BindGroupLayoutEntry {
                    binding,
                    visibility: ShaderStages::all(),
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: Some(min_binding_size),
                    },
                    count: None,
                });
                // We assume no recursion here. WGSL can't handle it anyway I think.
            }
            slang::BindingType::MutableRawBuffer | slang::BindingType::MutableTypedBuffer => {
                if !entries.contains_key(&current_set_index) {
                    entries.insert(current_set_index, vec![]);
                    next_set_index += 1;
                }
                let entry_vec = entries.get_mut(&current_set_index).unwrap();
                let binding = (entry_vec.len()).try_into().unwrap();
                let min_binding_size: NonZero<u64> = NonZero::new(
                    leaf_tl
                        .element_type_layout()
                        .size(slang::ParameterCategory::Uniform)
                        .try_into()
                        .unwrap(),
                )
                .unwrap();
                entry_vec.push(BindGroupLayoutEntry {
                    binding,
                    visibility: ShaderStages::all(),
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: Some(min_binding_size),
                    },
                    count: None,
                });
                // We assume no recursion here. WGSL can't handle it anyway I think.
            }
            _ => {
                unimplemented!()
            }
        }
    }
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

    pub fn navigate_into_var(&self, vl: &'a VariableLayout) -> Option<Cursor<'a>> {
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
    pub fn navigate_field(&self, field_index: u32) -> Option<Cursor<'a>> {
        let field_var = self.type_layout.field_by_index(field_index)?;
        self.navigate_into_var(field_var)
    }
    pub fn navigate_child(&self) -> Option<Cursor<'a>> {
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
    // Not safe, doesn't handle resource case correctly,
    // since many resources lack an element type (I think).
    pub fn navigate_index(&self, buffer_index: u32) -> Option<Cursor<'a>> {
        match self.type_layout.kind() {
            TypeKind::Array => {
                let element_tl = self.type_layout.element_type_layout();
                let set_stride = self
                    .type_layout
                    .element_stride(ParameterCategory::SubElementRegisterSpace);
                let slot_stride = self
                    .type_layout
                    .element_stride(ParameterCategory::DescriptorTableSlot);
                let uniform_stride = self.type_layout.element_stride(ParameterCategory::Uniform);
                let set_accumulator =
                    self.offset.set_accumulator + (buffer_index as usize) * set_stride;
                let set = if element_tl.kind() == TypeKind::ParameterBlock {
                    set_accumulator
                } else {
                    self.offset.set
                };
                let slot_accumulator = if set != self.offset.set {
                    0
                } else {
                    self.offset.slot_accumulator + (buffer_index as usize) * slot_stride
                };
                let slot = if set != self.offset.set {
                    0
                } else {
                    // The actual kind condition is [anything that can introduce its own slot], I think
                    match element_tl.kind() {
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
                    self.offset.uniform + (buffer_index as usize) * uniform_stride
                };
                Some(Cursor {
                    type_layout: element_tl,
                    offset: Offset {
                        set,
                        set_accumulator,
                        slot,
                        slot_accumulator,
                        uniform,
                    },
                })
            }
            TypeKind::Resource => {
                let element_tl = self.type_layout.element_type_layout();
                // Lol idk if the set and slot stride make any sense at all
                let set_stride = 0;
                let slot_stride = 0;
                let uniform_stride = self
                    .type_layout
                    .element_type_layout()
                    .stride(ParameterCategory::Uniform);
                let set_accumulator =
                    self.offset.set_accumulator + (buffer_index as usize) * set_stride;
                let set = if element_tl.kind() == TypeKind::ParameterBlock {
                    set_accumulator
                } else {
                    self.offset.set
                };
                let slot_accumulator = if set != self.offset.set {
                    0
                } else {
                    self.offset.slot_accumulator + (buffer_index as usize) * slot_stride
                };
                let slot = if set != self.offset.set {
                    0
                } else {
                    // The actual kind condition is [anything that can introduce its own slot], I think
                    match element_tl.kind() {
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
                    self.offset.uniform + (buffer_index as usize) * uniform_stride
                };
                Some(Cursor {
                    type_layout: element_tl,
                    offset: Offset {
                        set,
                        set_accumulator,
                        slot,
                        slot_accumulator,
                        uniform,
                    },
                })
            }
            _ => None,
        }
    }

    pub fn type_layout(&self) -> &TypeLayout {
        self.type_layout
    }

    pub fn offset(&self) -> Offset {
        self.offset
    }
}

pub fn walk_him_down(vl: &VariableLayout, c: Cursor) {
    println!(
        "{}:{};{:?}",
        vl.variable().map_or("<anon_var>", |x| { x.name() }),
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
