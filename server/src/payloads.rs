use actix_web::{HttpResponse, HttpResponseBuilder};
pub use common::payloads::*;
use serde::Serialize;

pub trait ToHttpResponse {
    /// Wraps an ErrorablePayload in an HttpResponse.
    /// on_successful is the builder used to construct an ErrorablePayload::Ok response.
    /// For example, you can set it to HttpResponse::Created() for 201 Created.
    fn to_response(self, on_successful: HttpResponseBuilder) -> HttpResponse;
}

impl<T: Serialize> ToHttpResponse for ErrorablePayload<T> {
    fn to_response(self, mut on_successful: HttpResponseBuilder) -> HttpResponse {
        match self {
            ErrorablePayload::Ok(_) => on_successful.json(self),
            ErrorablePayload::NotFound => HttpResponse::NotFound().json(self),
            ErrorablePayload::Err(_) => HttpResponse::InternalServerError().json(self),
        }
    }
}
