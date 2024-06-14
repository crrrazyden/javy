use crate::{
    hold, hold_and_release,
    quickjs::{context::Intrinsic, function::MutFn, qjs, Ctx,  TypedArray,Function,Error, Result},
    to_js_error,
};

#[allow(unused_imports)]
use std::alloc::{alloc, dealloc, Layout};
use crate::Args;
use anyhow::{ Error as AnyhowError, Result as AnyhowResult};
pub type CallbackFuncType = fn (i32, i32) -> i64;

pub struct Callback {}

//fn pack(a: i32, b: i32) -> i64 { (a as i64) << 32 | (b as i64) }
#[cfg(not(test))]
fn unpack(v: i64) -> (i32, i32) { ((v >> 32) as i32, (v & 0xffffffff) as i32) }

static mut CALLBACK: Option<CallbackFuncType> = None;

fn wrapper_callback<'js>(args: Args<'js>) -> AnyhowResult<TypedArray<'js, u8>, AnyhowError> {
    let (ctx, args) = args.release();
    if args.len() != 1 {
        return Err(AnyhowError::msg("wrong number of arguments: ".to_string() + &args.len().to_string()));
    }

    if args.get(0).unwrap().is_object() == false || args.get(0).unwrap().as_object().unwrap().is_typed_array::<u8>() == false {
        return Err(AnyhowError::msg("argument must be typed Uint8Array"));
    }

    let array_u8 = args.get(0).unwrap().as_object().unwrap().as_typed_array::<u8>().unwrap();
    let elems = array_u8.as_bytes().unwrap();
    let mem_pointer = &elems[0] as *const u8;

    #[cfg(not(test))]
    let func = unsafe { CALLBACK.unwrap() };
    #[cfg(not(test))]
    let (address, len) = unpack(func(mem_pointer as i32, elems.len() as i32));
    

    #[cfg(test)]
    let (address, len) = __callback(mem_pointer as i64, elems.len() as i32);

    let mem_pointer = address as *mut u8;
    if mem_pointer == std::ptr::null_mut() {
        return Err(AnyhowError::msg("result address is null"));
    }

    let array_u8 = unsafe { std::slice::from_raw_parts(mem_pointer, len as usize)};
    let res = TypedArray::<u8>::new(ctx, array_u8)?;

    unsafe {
        dealloc(mem_pointer, Layout::from_size_align(len as usize, 1).unwrap());
    }

    Ok(res)
}

#[cfg(not(test))]
fn callback(addr: i32, len: i32) -> i64 {
    unsafe { __callback(addr, len) }
}

#[cfg(not(test))]
extern "C" {
    pub fn __callback(val: i32, val: i32) -> i64;
}

impl Intrinsic for Callback {
    unsafe fn add_intrinsic(ctx: std::ptr::NonNull<qjs::JSContext>) {
        register(Ctx::from_raw(ctx)).expect("`Callback` APIs to succeed")
    }
}

impl Callback {
    pub fn enable() {
        #[cfg(not(test))]
        unsafe {
            CALLBACK = Some(callback);
        }

        #[cfg(test)]
        unsafe {
            let empty_func = |_,_| -> i64 { 0 };
            CALLBACK = Some(empty_func);
        }
    }
}

fn register(cx: Ctx) -> Result<()> {
    let globals = cx.globals();
    unsafe {
        match CALLBACK {
            Some(_) => {
                globals.set("callback", Function::new(
                    cx.clone(),
                    MutFn::new(move |cx, args| {
                        let (cx, args) = hold_and_release!(cx, args);
                        wrapper_callback(hold!(cx.clone(), args)).map_err(| e: AnyhowError | -> Error { 
                            if e.to_string().contains("wrong number") {
                                let got_arguments = e.to_string().split(":").collect::<Vec<&str>>()[1].trim().parse::<i32>().unwrap() as usize;
                                return Error::MissingArgs { expected: (1), given: (got_arguments) };
                            }
                            to_js_error(cx, e)
                        })
                    })))?;
                }
            None => {
                ()
            }
        }
    }

    Ok::<_, Error>(())
}

#[cfg(test)]
fn __callback(addr: i64, len: i32) -> (i64, i32) {
    let mempointer = addr as *mut u8;
    let array_u8 = unsafe { std::slice::from_raw_parts(mempointer, len as usize)};

    // for i in 0..len {
    //     println!("got value[{}]: {}",i, array_u8[i as usize]);
    // }
    
    let layout = Layout::from_size_align((len*2) as usize, 1);
    let alloc = unsafe { alloc(layout.unwrap()) };

    unsafe {
        for i in 0..(len*2) {
            if i < len {
                *(alloc.offset(i as isize) as *mut u8) = array_u8[i as usize]*2;    
            } else {
                *(alloc.offset(i as isize) as *mut u8) = (100+i) as u8;
            }
        }
    }
    (alloc as i64, len*2)
}

#[cfg(test)]
mod tests {

    use crate::{
        callback::*, quickjs::{context::EvalOptions, Error, Result, Value}, Runtime 
    };

    #[test]
    fn test_callback() -> Result<()> {
        let runtime = Runtime::default();
        Callback::enable();
        unsafe {Callback::add_intrinsic(runtime.context().as_raw());}

        runtime.context().with(|this| {

            let cmd1 = "callback(new Uint8Array([0,1,2,3,4,5,6,7,8,9]))";
            let res: Result<Value> = this.eval_with_options(cmd1, EvalOptions::default());
            assert!(res.as_ref().is_ok());
            assert!(res.as_ref().unwrap().is_object());
            assert!(res.as_ref().unwrap().as_object().unwrap().clone().is_typed_array::<u8>());
            let res_array = res.as_ref().unwrap().as_object().unwrap().as_typed_array::<u8>().unwrap();
            assert_eq!(res_array.as_bytes().unwrap(), [0,2,4,6,8,10,12,14,16,18,110,111,112,113,114,115,116,117,118,119]);

            let cmd2 = "callback()";
            let res: Result<Value> = this.eval_with_options(cmd2, EvalOptions::default());
            assert!(res.as_ref().is_err());
            assert!(res.as_ref().err().unwrap().is_exception());

            let cmd3 = "callback(2)";
            let res: Result<Value> = this.eval_with_options(cmd3, EvalOptions::default());
            assert!(res.as_ref().is_err());
            assert!(res.as_ref().err().unwrap().is_exception());
            
            Ok::<_, Error>(())
        })?;

        Ok(())
    }
}