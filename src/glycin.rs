//! Manual bindings to libglycin to avoid pulling in a lot of Rust deps.

use std::future::Future;
use std::ptr;

use glib::ffi::{gpointer, GError};
use glib::gobject_ffi::GObject;
use glib::translate::{from_glib_full, ToGlibPtr as _};
use gtk::gdk::ffi::GdkTexture;
use gtk::gio::ffi::{GAsyncReadyCallback, GAsyncResult, GCancellable, GFile};
use gtk::gio::{GioFuture, GioFutureResult};
use gtk::{gdk, gio};

#[link(name = "glycin-2", kind = "dylib")]
extern "C" {
    fn gly_loader_new(file: *mut GFile) -> *mut GObject;
    fn gly_loader_load_async(
        loader: *mut GObject,
        cancellable: *mut GCancellable,
        callback: GAsyncReadyCallback,
        user_data: gpointer,
    );
    fn gly_loader_load_finish(
        loader: *mut GObject,
        result: *mut GAsyncResult,
        error: *mut *mut GError,
    ) -> *mut GObject;
    fn gly_image_next_frame_async(
        image: *mut GObject,
        cancellable: *mut GCancellable,
        callback: GAsyncReadyCallback,
        user_data: gpointer,
    );
    fn gly_image_next_frame_finish(
        image: *mut GObject,
        result: *mut GAsyncResult,
        error: *mut *mut GError,
    ) -> *mut GObject;
}

#[link(name = "glycin-gtk4-2", kind = "dylib")]
extern "C" {
    fn gly_gtk_frame_get_texture(frame: *mut GObject) -> *mut GdkTexture;
}

pub struct Loader(glib::Object);
pub struct Image(glib::Object);
pub struct Frame(glib::Object);

pub fn loader_new(file: &gio::File) -> Loader {
    Loader(unsafe { from_glib_full(gly_loader_new(file.to_glib_none().0)) })
}

type ImageResult = Result<Image, glib::Error>;
pub fn loader_load_future(loader: &Loader) -> impl Future<Output = ImageResult> {
    GioFuture::new(
        &loader.0,
        |obj, cancellable, send: GioFutureResult<ImageResult>| unsafe {
            unsafe extern "C" fn trampoline(
                obj: *mut GObject,
                res: *mut GAsyncResult,
                user_data: gpointer,
            ) {
                let mut error = ptr::null_mut();
                let ret = gly_loader_load_finish(obj, res, &mut error);
                let result = if error.is_null() {
                    Ok(Image(from_glib_full(ret)))
                } else {
                    Err(from_glib_full(error))
                };
                let send: Box<GioFutureResult<ImageResult>> = Box::from_raw(user_data as *mut _);
                send.resolve(result);
            }

            let send = Box::new(send);
            gly_loader_load_async(
                obj.to_glib_none().0,
                cancellable.to_glib_none().0,
                Some(trampoline),
                Box::into_raw(send) as gpointer,
            );
        },
    )
}

type FrameResult = Result<Frame, glib::Error>;
pub fn image_next_frame_future(image: &Image) -> impl Future<Output = FrameResult> {
    GioFuture::new(
        &image.0,
        |obj, cancellable, send: GioFutureResult<FrameResult>| unsafe {
            unsafe extern "C" fn trampoline(
                obj: *mut GObject,
                res: *mut GAsyncResult,
                user_data: gpointer,
            ) {
                let mut error = ptr::null_mut();
                let ret = gly_image_next_frame_finish(obj, res, &mut error);
                let result = if error.is_null() {
                    Ok(Frame(from_glib_full(ret)))
                } else {
                    Err(from_glib_full(error))
                };
                let send: Box<GioFutureResult<FrameResult>> = Box::from_raw(user_data as *mut _);
                send.resolve(result);
            }

            let send = Box::new(send);
            gly_image_next_frame_async(
                obj.to_glib_none().0,
                cancellable.to_glib_none().0,
                Some(trampoline),
                Box::into_raw(send) as gpointer,
            );
        },
    )
}

pub fn frame_get_texture(frame: &Frame) -> gdk::Texture {
    unsafe { from_glib_full(gly_gtk_frame_get_texture(frame.0.to_glib_none().0)) }
}
