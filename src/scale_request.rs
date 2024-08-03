use glib::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScaleRequest {
    FitToAllocation,
    Set(f64),
}

impl From<ScaleRequest> for f64 {
    fn from(value: ScaleRequest) -> Self {
        match value {
            ScaleRequest::FitToAllocation => 0.,
            ScaleRequest::Set(scale) => scale,
        }
    }
}

impl From<f64> for ScaleRequest {
    fn from(value: f64) -> Self {
        if value == 0. {
            ScaleRequest::FitToAllocation
        } else {
            ScaleRequest::Set(value.clamp(0., 10.))
        }
    }
}

impl glib::HasParamSpec for ScaleRequest {
    type ParamSpec = glib::ParamSpecDouble;
    type SetValue = Self;
    type BuilderFn = fn(&str) -> glib::ParamSpecDoubleBuilder;

    fn param_spec_builder() -> Self::BuilderFn {
        Self::ParamSpec::builder
    }
}

impl From<ScaleRequest> for glib::Value {
    fn from(value: ScaleRequest) -> Self {
        value.to_value()
    }
}

impl glib::value::ToValue for ScaleRequest {
    fn to_value(&self) -> glib::Value {
        let value: f64 = (*self).into();
        value.to_value()
    }

    fn value_type(&self) -> glib::Type {
        f64::static_type()
    }
}

unsafe impl<'a> glib::value::FromValue<'a> for ScaleRequest {
    type Checker = glib::value::GenericValueTypeChecker<f64>;

    unsafe fn from_value(value: &'a glib::Value) -> Self {
        f64::from_value(value).into()
    }
}

impl Default for ScaleRequest {
    fn default() -> Self {
        Self::FitToAllocation
    }
}
