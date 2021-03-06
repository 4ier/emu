use crate::device::*;
use crate::error::*;
use crate::pool::*;

use std::collections::hash_map::{DefaultHasher, HashMap};
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::io::{Cursor, Read, Seek};
use std::sync::{Arc, RwLock};

#[cfg(feature = "spirv-compile")]
extern crate spirv_headers as spirv;
#[cfg(feature = "spirv-compile")]
extern crate spirv_reflect;
#[cfg(feature = "spirv-compile")]
use spirv::{AddressingModel, ExecutionModel, MemoryModel};
#[cfg(feature = "spirv-compile")]
use spirv_reflect::types::descriptor::ReflectDescriptorType;
#[cfg(feature = "spirv-compile")]
use spirv_reflect::*;

#[cfg(feature = "glsl-compile")]
extern crate spirv_headers as spirv;
#[cfg(feature = "glsl-compile")]
extern crate spirv_reflect;
#[cfg(feature = "glsl-compile")]
use shaderc::{};
#[cfg(feature = "glsl-compile")]
use spirv::{AddressingModel, ExecutionModel, MemoryModel};
#[cfg(feature = "glsl-compile")]
use spirv_reflect::types::descriptor::ReflectDescriptorType;
#[cfg(feature = "glsl-compile")]
use spirv_reflect::*;

//
// COMPILE
//

// Compile<I>: I -> (DeviceFnMutParams, String, &[u32])
// compile<I, Compile<I>, Cache> -> DeviceFnMut | (DeviceFnMutParams, String, &[u32])
// finish(*) -> DeviceFnMut

// a wrapper for SPIR-V code
// the wrapper adds a few important details including parameters and the name of the entry point to use in the code
pub struct Spirv<T: Borrow<[u32]> {
    pub params: DeviceFnMutParams,
    pub name: String,
    pub code: T
}

// implement this trait for any compiler of high-level source language to SPIR-V
// the compiler should be able to output a module with single entry point (i.e. - a single compute kernel)
//
// generic over its inpput
pub trait Compile<I: Hash> {
    fn compile(src: I) -> Result<Spirv, CompileError>;
}

// // this doesn't support **all** SPIR-V
// // rather, it only supports SPIR-V programs that contain a single compute (GL) kernel and only a single descriptor set which contains only storage buffers
// //
// // so if you're using SpirvCompile to compile from a high-level program representation down to SPIR-V or even to hand-write SPIR-V, there are 3 things you should ensure
// // 1. your SPIR-V program only has a single entrypoint - that of a GL compute kernel
// // 2. you only use a single description set
// // 3. all the bindings inside that description set are storage buffers (no push constants, just use buffers)
// //
// // by having these restrictions, we can use DeviceFnMut as a compact way of expressing a compute kernel
// #[cfg(feature = "spirv-compile")]
// pub struct SpirvCompile;

// #[cfg(feature = "spirv-compile")]
// impl Compile<Vec<u8>> for SpirvCompile {
//     type Output = Cursor<Vec<u8>>;

//     fn compile(
//         mut src: Vec<u8>,
//     ) -> Result<(DeviceFnMutParams, String, Self::Output), CompileError> {
//         let mut num_params = 0;
//         let mut kernel_name = String::new();

//         // // attempt using rspirv
//         // let mut loader = Loader::new();
//         // {
//         //     let p = Parser::new(&src_into, &mut loader);
//         //     p.parse().unwrap();
//         // }
//         // let module = loader.module();

//         // attempt using spirv-reflect-rs
//         let module = unsafe { ShaderModule::load_u8_data(&src).unwrap() };
//         let entry_point = module
//             .enumerate_entry_points()
//             .unwrap()
//             .iter()
//             .find(|entry_point| entry_point.spirv_execution_model == ExecutionModel::GLCompute)?
//             .clone();
//         kernel_name = entry_point.name.clone();
//         num_params = entry_point.descriptor_sets[0]
//             .bindings
//             .iter()
//             .filter(|binding| binding.descriptor_type == ReflectDescriptorType::StorageBuffer)
//             .count();

//         Ok((
//             DeviceFnMutParams::new(num_params),
//             kernel_name,
//             Cursor::new(src),
//         ))
//     }
// }

// // once again, we have requirements for what kind of GLSL works
// // all the requirements from above apply as well as a fourth one
// // 4. the name of the entry point must be "main"
// #[cfg(feature = "glsl-compile")]
// pub struct GlslCompile;

// #[cfg(feature = "glsl-compile")]
// impl Compile<String> for GlslCompile {
//     type Output = Cursor<Vec<u8>>;

//     fn compile(mut src: String) -> Result<(DeviceFnMutParams, String, Self::Output), CompileError> {
//         let mut num_params = 0;
//         let mut kernel_name = String::new();

//         // (1) compile to SPIR-V
//         let mut compiler = unsafe { shaderc::Compiler::new().unwrap() };
//         let binary_result = unsafe {
//             compiler
//                 .compile_into_spirv(
//                     &src,
//                     shaderc::ShaderKind::Compute,
//                     "a compute shader",
//                     "main",
//                     None,
//                 )
//                 .unwrap()
//         };
//         let spirv_bytes = Vec::from(binary_result.as_binary_u8());

//         // (2) extract info

//         let module = unsafe { ShaderModule::load_u8_data(&spirv_bytes).unwrap() };
//         let entry_point = module
//             .enumerate_entry_points()
//             .unwrap()
//             .iter()
//             .find(|entry_point| entry_point.spirv_execution_model == ExecutionModel::GLCompute)
//             .ok_or(CompileError)?
//             .clone();
//         kernel_name = entry_point.name.clone();
//         num_params = entry_point.descriptor_sets[0]
//             .bindings
//             .iter()
//             .filter(|binding| binding.descriptor_type == ReflectDescriptorType::StorageBuffer)
//             .count();

//         Ok((
//             DeviceFnMutParams::new(num_params),
//             kernel_name,
//             Cursor::new(spirv_bytes),
//         ))
//     }
// }

// TODO create GlslKernel { local_size, structs, parameters, helper_code, kernel_code}
// TODO add trait, derive macro for structs to GLSL

#[derive(Hash)]
pub struct GlslKernel {
    code: String,
    params: Vec<String>,
    params_mutability: Vec<Mutability>,
    structs: Vec<String>,
    local_size: Vec<u32>,
    helper_code: String,
    kernel_code: String,
}

impl GlslKernel {
    pub fn new() -> Self {
        Self {
            code: String::from("#version 450\n"),
            params: vec![],
            params_mutability: vec![],
            structs: vec![],
            local_size: vec![],
            helper_code: String::new(),
            kernel_code: String::new(),
        }
    }

    pub fn spawn(mut self, num_threads: u32) -> Self {
        if self.local_size.len() == 3 {
            panic!("cannot spawn more threads within thread block");
        }
        self.local_size.push(num_threads);
        self
    }

    pub fn with_struct<T: GlslStruct>(mut self) -> Self {
        self.structs.push(T::as_glsl());
        self
    }

    pub fn param(mut self, param: impl Into<String>) -> Self {
        self.params.push(param.into());
        self.params_mutability.push(Mutability::Const);
        self
    }

    pub fn param_mut(mut self, param: impl Into<String>) -> Self {
        self.params.push(param.into());
        self.params_mutability.push(Mutability::Mut);
        self
    }

    pub fn with_helper_code(mut self, code: impl Into<String>) -> Self {
        self.helper_code = code.into();
        self
    }

    pub fn with_kernel_code(mut self, code: impl Into<String>) -> Self {
        self.kernel_code = code.into();
        self
    }
}

#[cfg(feature = "glsl-compile")]
pub struct GlslKernelCompile;

#[cfg(feature = "glsl-compile")]
impl Compile<GlslKernel> for GlslKernelCompile {
    type Output = Cursor<Vec<u8>>;

    fn compile(
        mut src: GlslKernel,
    ) -> Result<(DeviceFnMutParams, String, Self::Output), CompileError> {
        let mut num_params = src.params.len();
        let mut kernel_name = String::from("main");

        // (1) local size
        if src.local_size.len() == 0 {
            src.local_size = vec![1];
        }
        src.code += "\nlayout(";
        if src.local_size.len() == 1 {
            src.code += "local_size_x = ";
            src.code += &src.local_size[0].to_string();
        }
        if src.local_size.len() == 2 {
            src.code += "local_size_x = ";
            src.code += &src.local_size[0].to_string();
            src.code += ", local_size_y = ";
            src.code += &src.local_size[1].to_string();
        }
        if src.local_size.len() == 3 {
            src.code += "local_size_x = ";
            src.code += &src.local_size[0].to_string();
            src.code += ", local_size_y = ";
            src.code += &src.local_size[1].to_string();
            src.code += ", local_size_z = ";
            src.code += &src.local_size[2].to_string();
        }
        src.code += ") in;\n";

        // (2) structs
        for struct_def in src.structs {
            src.code += &struct_def;
        }

        // (3) buffer for each parameter
        let mut params = ParamBuilder::new();
        for (i, param) in src.params.iter().enumerate() {
            params = params.param(src.params_mutability[i]);
            src.code += "\nlayout(set = 0, binding = ";
            src.code += &i.to_string();
            src.code += ") buffer Buffer";
            src.code += &i.to_string();
            src.code += " {\n";
            src.code += param;
            src.code += ";\n};\n";
        }

        // (4) helper code
        src.code += &src.helper_code;

        // (5) kernel code
        src.code += "\nvoid main() {\n";
        src.code += &src.kernel_code;
        src.code += "}\n";

        // (6) compile to SPIR-V
        let mut compiler = unsafe { shaderc::Compiler::new().unwrap() };
        let binary_result = unsafe {
            compiler
                .compile_into_spirv(
                    &src.code,
                    shaderc::ShaderKind::Compute,
                    "a compute shader",
                    "main",
                    None,
                )
                .unwrap()
        };
        let spirv_bytes = Vec::from(binary_result.as_binary_u8());

        Ok(Spirv {
            params: params.build(), 
            name: kernel_name, Cursor::new(spirv_bytes)))
    }
}

//
// CACHE
//



//
// FUNCTIONS
//

pub enum MaybeDeviceFnMut<C: Cache> {
    Spirv((Spirv, u64)), // we need to pass in the hash of the source so that we can store the finished result in the cache
    Finished(Arc<DeviceFnMut>)
}

impl MaybeDeviceFnMut {
    pub fn get_spirv_code_mut(&mut self) -> Option<&mut [u32]> {
        match self {
            Spirv((spirv, src_hash)) => {
                Some(&mut spirv.code)
            }
            _ => None
        }
    }

    pub fn finish(&self) -> Result<Arc<DeviceFnMut>, CompileOrNoDeviceError> {
        match self {
            Spirv((spirv, src_hash)) => {
                C::insert(
                    src_hash,
                    Arc::new(
                        take()
                            .map_err(|_| CompileOrNoDeviceError::NoDevice)?
                            .lock()
                            .unwrap()
                            .compile::<_>(spirv.params, spirv.name, spirv.code)
                            .map_err(|_| CompileOrNoDeviceError::Compile)?,
                    ),
                );
                Ok(C::get(src_hash))
            }
            Finished(device_fn_mut) => Ok(device_fn_mut)
        }
    }
}

// TODO add a module for easily caching kernels
// TODO instead of Compile and compile, create module for dynamically defining kernels (or maybe even computation with kernels cached)

// Var, Const, Type (derived from structs), Assign, ForLoop, WhileLoop
// goal: a low-level thin abstraction over GLSL syntax using Rusty types

// this module only provides an API for simple JIT compilation from a source language
// in fact, it doesn't even do the JIT itself (relies on a trait implemented elsewhere)
// and stuff like providing macros that do AOT compilation of structs, functions won't be in this module
// just simple interface for JIT compilation

// this function returns a scary looking Arc (automatic reference counting pointer)
// but you can easily convert it to a reference with &
// we use Arc because DeviceFnMut's are loaded from global caches that can be used from various different threads
//
// this function is safe as long as U can compile to SPIR-V safely
// once U compiles to SPIR-V, wgpu-rs should be able to safely compile to machine code
pub fn compile<I: Hash, U: Compile<I>, C: Cache>(src: I) -> Result<MaybeDeviceFnMut<C>, CompileError> {
    let src_into = src;

    // hash to check in cache
    let mut hasher = DefaultHasher::new();
    src_into.hash(&mut hasher);
    let hash = hasher.finish();

    if C::contains(hash) {
        Ok(MaybeSpirv::Finished(C::get(hash)))
    } else {
        let spirv = U::compile(src_into)?;
        Ok(MaybeSpirv::Spirv((spirv, hash)))
    }
}

// pub fn compile<I: Hash, U: Compile<I, Output = O>, O, C: Cache>(
//     src: I,
// ) -> Result<Arc<DeviceFnMut>, CompileOrNoDeviceError>
// where
//     O: Read + Seek,
// {
    // let src_into = src; //.into();

    // // hash to check in cache
    // let mut hasher = DefaultHasher::new();
    // src_into.hash(&mut hasher);
    // let hash = hasher.finish();

    // if C::contains(hash) {
    //     Ok(C::get(hash))
    // } else {
    //     let compiled = U::compile(src_into).map_err(|_| CompileOrNoDeviceError::Compile)?;
    //     C::insert(
    //         hash,
    //         Arc::new(
    //             take()
    //                 .map_err(|_| CompileOrNoDeviceError::NoDevice)?
    //                 .lock()
    //                 .unwrap()
    //                 .compile::<String, _>(compiled.0, compiled.2, compiled.1)
    //                 .map_err(|_| CompileOrNoDeviceError::Compile)?,
    //         ),
    //     );
    //     Ok(C::get(hash))
    // }
// }

// TODO add function to compile SPIR-V to DeviceFnMut
// TODO add ability to use RAM-based cache
// TODO add ability to use disk-based cache