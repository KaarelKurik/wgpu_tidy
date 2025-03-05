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

use std::collections::HashMap;

use bytemuck::Contiguous;
use slang::reflection::{TypeLayout, VariableLayout};
use wgpu::{
    BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry, ShaderStages,
    TextureViewDimension,
};

#[derive(Default, Debug, Clone, Copy)]
struct QuasiOffset {
    sub_er_space: usize,
    sub_er_space_accumulator: usize,
    value: usize,
}

// Correctness:
// category != SubElementRegisterSpace, RegisterSpace, DescriptorTableSlot
#[derive(Debug, Clone, Copy)]
pub struct SimpleOffset {
    category: slang::ParameterCategory,
    value: usize,
}

impl SimpleOffset {
    pub fn new(category: slang::ParameterCategory) -> Self {
        match category {
            slang::ParameterCategory::DescriptorTableSlot
            | slang::ParameterCategory::RegisterSpace
            | slang::ParameterCategory::SubElementRegisterSpace => panic!(),
            _ => {}
        }
        return SimpleOffset { category, value: 0 };
    }
}

// TODO: also make one of these for DescriptorSlot
#[derive(Debug, Clone, Copy, Default)]
pub struct SubEROffset {
    value: usize,
    accumulator: usize,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DescriptorTableSlotOffset {
    value: usize,
    weak_parent_container_var_offset: usize,
    pure_increment: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum Offset {
    SimpleOffset(SimpleOffset),
    SubEROffset(SubEROffset),
    DescriptorTableSlotOffset(DescriptorTableSlotOffset),
}

// For some reason, every layout unit has its own
// sub_er_space to track. I don't understand why.
// Then again, the Discord message concerning
// the unreliability of getBindingSpace (even with a parameter)
// suggests that the simple logic of computing offsets by direct
// addition is the correct way to go, rather than however
// I was doing it before.
#[derive(Debug, Clone)]
struct Offsets {
    offsets: HashMap<slang::ParameterCategory, QuasiOffset>,
}

fn increment_slot_offset(
    vl: &VariableLayout,
    parent_absolute_offset: DescriptorTableSlotOffset,
) -> DescriptorTableSlotOffset {
    let tl = vl.type_layout();
    if singleton_kind(tl.kind())
        && tl
            .container_var_layout()
            .type_layout()
            .size(slang::ParameterCategory::SubElementRegisterSpace)
            > 0
    {
        let value = 0;
        let weak_parent_container_var_offset = 0;
        let pure_increment = 0;
        DescriptorTableSlotOffset {
            value,
            weak_parent_container_var_offset,
            pure_increment
        }
    } else {
        let pure_increment = parent_absolute_offset.pure_increment + vl.offset(slang::ParameterCategory::DescriptorTableSlot);
        let weak_parent_container_var_offset = if singleton_kind(tl.kind()) {
            pure_increment
        } else {
            parent_absolute_offset.weak_parent_container_var_offset
        };
        let value = match tl.kind() {
            slang::TypeKind::None => todo!(),
            slang::TypeKind::Struct => todo!(),
            slang::TypeKind::Array => todo!(),
            slang::TypeKind::Matrix => todo!(),
            slang::TypeKind::Vector => todo!(),
            slang::TypeKind::Scalar => todo!(),
            slang::TypeKind::ConstantBuffer => todo!(),
            slang::TypeKind::Resource => todo!(),
            slang::TypeKind::SamplerState => todo!(),
            slang::TypeKind::TextureBuffer => todo!(),
            slang::TypeKind::ShaderStorageBuffer => todo!(),
            slang::TypeKind::ParameterBlock => todo!(),
            slang::TypeKind::GenericTypeParameter => todo!(),
            slang::TypeKind::Interface => todo!(),
            slang::TypeKind::OutputStream => todo!(),
            slang::TypeKind::MeshOutput => todo!(),
            slang::TypeKind::Specialized => todo!(),
            slang::TypeKind::Feedback => todo!(),
            slang::TypeKind::Pointer => todo!(),
            slang::TypeKind::DynamicResource => todo!(),
            slang::TypeKind::Count => todo!(),
        };
    }
}

fn increment_sub_er_offset(
    vl: &VariableLayout,
    parent_absolute_offset: SubEROffset,
) -> SubEROffset {
    let tl = vl.type_layout();
    let accumulator = parent_absolute_offset.accumulator
        + vl.offset(slang::ParameterCategory::SubElementRegisterSpace);
    let value = if singleton_kind(tl.kind())
        && tl
            .container_var_layout()
            .type_layout()
            .size(slang::ParameterCategory::SubElementRegisterSpace)
            > 0
    {
        accumulator
    } else {
        parent_absolute_offset.value
    };
    SubEROffset { accumulator, value }
}

fn increment_simple_offset(
    vl: &VariableLayout,
    parent_absolute_offset: SimpleOffset,
) -> SimpleOffset {
    let tl = vl.type_layout();
    let category = parent_absolute_offset.category;
    match category {
        slang::ParameterCategory::Uniform => {
            let value = if singleton_kind(tl.kind()) {
                0
            } else {
                parent_absolute_offset.value + vl.offset(category)
            };
            SimpleOffset { value, category }
        }
        slang::ParameterCategory::ConstantBuffer
        | slang::ParameterCategory::ShaderResource
        | slang::ParameterCategory::UnorderedAccess
        | slang::ParameterCategory::SamplerState
        | slang::ParameterCategory::DescriptorTableSlot => {
            let value = if singleton_kind(tl.kind())
                && tl
                    .container_var_layout()
                    .type_layout()
                    .size(slang::ParameterCategory::SubElementRegisterSpace)
                    > 0
            {
                0
            } else {
                parent_absolute_offset.value + vl.offset(category)
            };
            SimpleOffset { value, category }
        }
        _ => SimpleOffset {
            value: parent_absolute_offset.value + vl.offset(category),
            category,
        },
    }
}

fn increment_offset(vl: &VariableLayout, parent_absolute_offset: Offset) -> Offset {
    match parent_absolute_offset {
        Offset::SimpleOffset(simple_offset) => {
            Offset::SimpleOffset(increment_simple_offset(vl, simple_offset))
        }
        Offset::SubEROffset(sub_eroffset) => {
            Offset::SubEROffset(increment_sub_er_offset(vl, sub_eroffset))
        }
        Offset::DescriptorTableSlotOffset(descriptor_table_slot_offset) => {
            Offset::DescriptorTableSlotOffset(increment_slot_offset(
                vl,
                descriptor_table_slot_offset,
            ))
        }
    }
}

pub fn walk_increment_print(vl: &VariableLayout, parent_absolute_offset: Offset) {
    fn helper(vl: &VariableLayout, parent_absolute_offset: Offset, lvl: usize) {
        let padding = "  ".repeat(lvl);
        let current_offset = increment_offset(vl, parent_absolute_offset);
        println!(
            "{}{} : {} ; {:?}",
            padding,
            vl.variable().name().unwrap_or("<anon_var>"),
            vl.type_layout().name().unwrap_or("<anon_type>"),
            vl.type_layout().kind()
        );
        println!("{}offset: {:?}", padding, current_offset);
        for child in get_children(vl) {
            helper(child, current_offset, lvl + 1);
        }
    }
    helper(vl, parent_absolute_offset, 0);
}

pub struct VarTree<'a> {
    head: &'a VariableLayout,
    children: Vec<VarTree<'a>>,
    cum_offset: QuasiOffset,
}

// Reminder: don't match by container type,
// it's none for ParameterBlocks even if they have
// POD data.
fn extend_layout_entries(
    x: &VariableLayout,
    entries: &mut HashMap<usize, Vec<BindGroupLayoutEntry>>,
    slot_offset: SimpleOffset, // current, not parent
    sub_er_offset: SubEROffset,
) {
    let set_index = sub_er_offset.value;
    let range_index = slot_offset.value;
    let tl = x.type_layout();
    if !entries.contains_key(&set_index) {
        entries.insert(set_index, vec![]);
    }
    let ce = entries.get_mut(&sub_er_offset.value).unwrap();
    // Is the method I call actually local, or does type layout store global info?
    // Unclear, may cause a bug.
    let binding_type = match tl.descriptor_set_descriptor_range_type(
        set_index.try_into().unwrap(),
        range_index.try_into().unwrap(),
    ) {
        slang::BindingType::Sampler => {
            // Requires extra information from the wgpu side, I think
            unimplemented!()
        }
        slang::BindingType::Texture => {
            // TODO: check whether this works at all
            let view_dimension = match x.ty().resource_shape() {
                slang::ResourceShape::SlangTexture1d => wgpu::TextureViewDimension::D1,
                slang::ResourceShape::SlangTexture2d => wgpu::TextureViewDimension::D2,
                slang::ResourceShape::SlangTexture3d => wgpu::TextureViewDimension::D3,
                slang::ResourceShape::SlangTextureCube => wgpu::TextureViewDimension::Cube,
                slang::ResourceShape::SlangTexture2dArray => wgpu::TextureViewDimension::D2Array,
                slang::ResourceShape::SlangTextureCubeArray => wgpu::TextureViewDimension::D3,
                _ => {
                    unreachable!()
                }
            };
            let b = wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: view_dimension,
                multisampled: false,
            };
            b
        }
        slang::BindingType::ConstantBuffer => {
            let b = wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            };
            b
        }
        slang::BindingType::ParameterBlock => {
            // Dunno if this needs to be done also
            todo!()
        }
        slang::BindingType::RawBuffer => {
            let b = wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            };
            b
        }
        slang::BindingType::MutableRawBuffer => {
            let b = wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            };
            b
        }
        _ => unimplemented!(),
    };
    let bgle = BindGroupLayoutEntry {
        binding: slot_offset.value.try_into().unwrap(),
        visibility: ShaderStages::all(),
        ty: binding_type,
        count: None,
    };
    ce.push(bgle);
}

pub fn print_var_layout(x: &VariableLayout) {
    fn helper(x: &VariableLayout, lvl: usize) {
        let padding = "  ".repeat(lvl);
        println!(
            "{}{}: {} ; {:?}",
            padding,
            x.variable().name().unwrap_or("<anon_var>"),
            x.type_layout().name().unwrap_or("<anon_type>"),
            x.type_layout().kind()
        );
        let category_count = x.category_count();
        println!("{} offsets", padding);
        for i in 0..category_count {
            let category = x.category_by_index(i);
            println!("{} - {:?}: {}", padding, category, x.offset(category),);
        }
        // println!(
        // 	"{} - {:?}: {}",
        // 	padding,
        // 	slang::ParameterCategory::RegisterSpace,
        // 	x.offset(slang::ParameterCategory::RegisterSpace)
        // );
        let tl = x.type_layout();
        let binding_range_count = tl.binding_range_count();
        println!("binding range count: {:?}", binding_range_count);
        for i in 0..binding_range_count {
            let brbc = tl.binding_range_binding_count(i);
            let brt = tl.binding_range_type(i);
            let sub_tl = tl.binding_range_leaf_type_layout(i);
            let dsi = tl.binding_range_descriptor_set_index(i);
            let rs = sub_tl.resource_shape();
            let fdri = tl.binding_range_first_descriptor_range_index(i);
            println!("{}: {}, {:?}", i, brbc, sub_tl.name());
            println!("descriptor set index: {}", dsi);
            println!("binding range type: {:?}", brt);
            println!("subtl resource shape: {:?}", rs);
            println!("first descriptor range index: {:?}", fdri);
        }
        println!("{} sizes", padding);
        for i in 0..category_count {
            let category = x.category_by_index(i);
            println!(
                "{} - {:?}: {}",
                padding,
                category,
                x.type_layout().size(category)
            );
        }
        if x.type_layout().field_count() > 0 {
            println!("{}recursing into fields", padding);
        }
        for ele in x.type_layout().fields().into_iter() {
            helper(ele, lvl + 1);
        }
        match x.type_layout().kind() {
            slang::TypeKind::ConstantBuffer
            | slang::TypeKind::ShaderStorageBuffer
            | slang::TypeKind::ParameterBlock
            | slang::TypeKind::TextureBuffer => {
                println!("{}recursing into container var layout", padding);
                helper(x.type_layout().container_var_layout(), lvl + 1);
                println!("{}recursing into element var layout", padding);
                helper(x.type_layout().element_var_layout(), lvl + 1);
            }
            _ => {}
        }
    }
    helper(x, 0);
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
                unimplemented!()
            }
            slang::BindingType::Texture => {
                if !entries.contains_key(&current_set_index) {
                    entries.insert(current_set_index, vec![]);
                    next_set_index += 1;
                }
                let entry_vec = entries.get_mut(&current_set_index).unwrap();
                let binding = (entry_vec.len()).try_into().unwrap();
                let view_dimension = match leaf_tl.resource_shape() {
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
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension,
                        multisampled: false,
                    },
                    count: None,
                });
            }
            slang::BindingType::ConstantBuffer => {
                println!("leaf_tl: {:?}", leaf_tl.kind());
                if leaf_tl
                    .container_var_layout()
                    .type_layout()
                    .size(slang::ParameterCategory::DescriptorTableSlot)
                    > 0
                {
                    println!("why not?");
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
                            min_binding_size: None,
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
                let current_set_index = next_set_index;
                entries.insert(current_set_index, vec![]);
                next_set_index += 1;
                if leaf_tl
                    .container_var_layout()
                    .type_layout()
                    .size(slang::ParameterCategory::DescriptorTableSlot)
                    > 0
                {
                    let entry_vec = entries.get_mut(&current_set_index).unwrap();
                    let binding: u32 = entry_vec.len().try_into().unwrap();
                    entry_vec.push(BindGroupLayoutEntry {
                        binding,
                        visibility: ShaderStages::all(),
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
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
            slang::BindingType::RawBuffer => {
                if !entries.contains_key(&current_set_index) {
                    entries.insert(current_set_index, vec![]);
                    next_set_index += 1;
                }
                let entry_vec = entries.get_mut(&current_set_index).unwrap();
                let binding = (entry_vec.len()).try_into().unwrap();
                entry_vec.push(BindGroupLayoutEntry {
                    binding,
                    visibility: ShaderStages::all(),
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                });
                // We assume no recursion here. WGSL can't handle it anyway I think.
            }
            slang::BindingType::MutableRawBuffer => {
                if !entries.contains_key(&current_set_index) {
                    entries.insert(current_set_index, vec![]);
                    next_set_index += 1;
                }
                let entry_vec = entries.get_mut(&current_set_index).unwrap();
                let binding = (entry_vec.len()).try_into().unwrap();
                entry_vec.push(BindGroupLayoutEntry {
                    binding,
                    visibility: ShaderStages::all(),
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
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

fn assoc_bind_group_layout(x: &VariableLayout, d: wgpu::Device) -> HashMap<usize, BindGroupLayout> {
    let mut out = HashMap::new();
    let entries = vec![];
    let bgld = BindGroupLayoutDescriptor {
        label: None,
        entries: entries.as_slice(),
    };
    let bgl = d.create_bind_group_layout(&bgld);
    out.insert(0, bgl);
    out
}

fn get_children(x: &VariableLayout) -> Vec<&VariableLayout> {
    let tl = x.type_layout();
    match tl.kind() {
        slang::TypeKind::ConstantBuffer
        | slang::TypeKind::ShaderStorageBuffer
        | slang::TypeKind::ParameterBlock
        | slang::TypeKind::TextureBuffer => {
            vec![tl.element_var_layout()]
        }
        _ => tl.fields().collect(),
    }
}
fn singleton_kind(k: slang::TypeKind) -> bool {
    match k {
        slang::TypeKind::ConstantBuffer
        | slang::TypeKind::ShaderStorageBuffer
        | slang::TypeKind::ParameterBlock
        | slang::TypeKind::TextureBuffer => true,
        _ => false,
    }
}

// I think this is correct except when c == SubElementRegisterSpace
// But it calculates the SERS offset correctly into 'space' anyway
pub fn var_tree<'a>(x: &'a VariableLayout, c: slang::ParameterCategory) -> VarTree<'a> {
    fn helper<'a>(
        x: &'a VariableLayout,
        c: slang::ParameterCategory,
        cum_off_at_parent: QuasiOffset,
    ) -> VarTree<'a> {
        let tl = x.type_layout();
        let children = get_children(x);
        let cum_off_here: QuasiOffset = match c {
            slang::ParameterCategory::Uniform => {
                let value = if singleton_kind(tl.kind()) {
                    0
                } else {
                    cum_off_at_parent.value + x.offset(c)
                };
                QuasiOffset {
                    value,
                    ..Default::default()
                }
            }
            slang::ParameterCategory::ConstantBuffer
            | slang::ParameterCategory::ShaderResource
            | slang::ParameterCategory::UnorderedAccess
            | slang::ParameterCategory::SamplerState
            | slang::ParameterCategory::DescriptorTableSlot => {
                let sub_er_space_accumulator = cum_off_at_parent.sub_er_space_accumulator
                    + x.offset(slang::ParameterCategory::SubElementRegisterSpace);
                if singleton_kind(tl.kind())
                    && tl
                        .container_var_layout()
                        .type_layout()
                        .size(slang::ParameterCategory::SubElementRegisterSpace)
                        > 0
                {
                    QuasiOffset {
                        sub_er_space: sub_er_space_accumulator,
                        sub_er_space_accumulator,
                        value: 0,
                    }
                } else {
                    QuasiOffset {
                        sub_er_space: cum_off_at_parent.sub_er_space
                            + x.binding_space_with_category(c),
                        sub_er_space_accumulator,
                        value: cum_off_at_parent.value + x.offset(c),
                    }
                }
            }
            slang::ParameterCategory::SubElementRegisterSpace => {
                let sub_er_space_accumulator = cum_off_at_parent.sub_er_space_accumulator
                    + x.offset(slang::ParameterCategory::SubElementRegisterSpace);
                if singleton_kind(tl.kind())
                    && tl
                        .container_var_layout()
                        .type_layout()
                        .size(slang::ParameterCategory::SubElementRegisterSpace)
                        > 0
                {
                    QuasiOffset {
                        sub_er_space: sub_er_space_accumulator,
                        sub_er_space_accumulator,
                        value: sub_er_space_accumulator,
                    }
                } else {
                    let sub_er_space =
                        cum_off_at_parent.sub_er_space + x.binding_space_with_category(c);
                    QuasiOffset {
                        sub_er_space,
                        sub_er_space_accumulator,
                        value: sub_er_space,
                    }
                }
            }
            _ => QuasiOffset {
                value: cum_off_at_parent.value + x.offset(c),
                ..Default::default()
            },
        };
        let child_trees: Vec<VarTree<'_>> = children
            .iter()
            .map(|x| helper(x, c, cum_off_here))
            .collect();
        VarTree {
            head: x,
            children: child_trees,
            cum_offset: cum_off_here,
        }
    }
    helper(x, c, QuasiOffset::default())
}

pub fn print_var_tree(t: &VarTree<'_>) {
    fn helper(t: &VarTree<'_>, lvl: usize) {
        let head_name = t.head.variable().name().unwrap_or("<anon_var>");
        let head_type_name = t.head.type_layout().name().unwrap_or("<anon_type>");
        let tabs = "\t".repeat(lvl);
        let kind = t.head.type_layout().kind();
        println!(
            "{}{}: {} ; {:?} with offset {:?}",
            tabs, head_name, head_type_name, kind, t.cum_offset
        );
        for child in &t.children {
            helper(child, lvl + 1);
        }
    }
    helper(t, 0);
}
