//! Service container for grouping handlers into API namespaces.
//!
//! A [`Service`] has a name, description, and a tree of [`Handler`] nodes.
//! Multiple services can be registered on an [`AFast`](crate::AFast) application,
//! and each service generates a separate client code file.

use crate::handler::Handler;
#[cfg(feature = "ordinary-http")]
use crate::handler::HandlerEntry;

/// Information about a registered ordinary HTTP route within a service.
///
/// Stores the HTTP method, path, and handler entry for an ordinary
/// (non-binary) route, enabling the server to build a routing table.
#[cfg(feature = "ordinary-http")]
#[derive(Clone)]
pub struct OrdinaryRouteInfo {
    /// HTTP method for this route (e.g. `"GET"`, `"POST"`).
    pub method: &'static str,
    /// Route path (e.g. `"/users/:id"`).
    pub path: String,
    /// The handler entry containing name, invoker, metadata, and ordinary invoker.
    pub handler_entry: HandlerEntry,
    /// The service this route belongs to.
    pub service_name: String,
}

/// A named group of handlers representing an API service.
///
/// Each service has a name, description, and a tree of [`Handler`] nodes.
/// Multiple services can be registered on an [`AFast`](crate::AFast) application,
/// and each service generates a separate client code file.
#[derive(Clone)]
pub struct Service {
    /// Service name, used as the client class name and file name.
    pub name: String,
    /// Human-readable description of the service.
    pub desc: String,
    /// Root handler nodes (may be groups or leaves).
    pub handlers: Vec<Handler>,
    /// Ordinary HTTP routes registered in this service.
    #[cfg(feature = "ordinary-http")]
    pub ordinary_routes: Vec<OrdinaryRouteInfo>,
    /// Ordinary WebSocket routes registered in this service.
    #[cfg(feature = "ordinary-ws")]
    pub ws_routes: Vec<crate::app::ordinary_ws::WsRouteInfo>,
    /// Ordinary SSE routes registered in this service.
    #[cfg(feature = "ordinary-sse")]
    pub sse_routes: Vec<crate::app::ordinary_sse::SseRouteInfo>,
    /// Per-service lifecycle hooks. When non-empty, these override global
    /// hooks for handlers belonging to this service.
    #[cfg(feature = "hook")]
    pub hooks: Vec<std::sync::Arc<dyn crate::hook::Hook>>,
}

impl Service {
    /// Creates a new service with the given name.
    pub fn new(name: impl AsRef<str>) -> Self {
        Self {
            name: name.as_ref().to_string(),
            desc: String::new(),
            handlers: Vec::new(),
            #[cfg(feature = "ordinary-http")]
            ordinary_routes: Vec::new(),
            #[cfg(feature = "ordinary-ws")]
            ws_routes: Vec::new(),
            #[cfg(feature = "ordinary-sse")]
            sse_routes: Vec::new(),
            #[cfg(feature = "hook")]
            hooks: Vec::new(),
        }
    }

    /// Sets the service description.
    #[doc(hidden)]
    pub fn desc(mut self, desc: impl Into<String>) -> Self {
        self.desc = desc.into();
        self
    }

    /// Adds a handler node (group or leaf) to this service.
    #[doc(hidden)]
    pub fn handler(mut self, handler: Handler) -> Self {
        self.handlers.push(handler);
        self
    }

    /// Registers a lifecycle hook for this service.
    ///
    /// Service hooks run **after** global hooks (registered via
    /// [`AFast::hook()`](crate::AFast::hook)) for all handlers belonging
    /// to this service.  Both global and service hooks always execute;
    /// they never replace each other.
    #[cfg(feature = "hook")]
    pub fn hook(mut self, hook: impl crate::hook::Hook + 'static) -> Self {
        self.hooks.push(std::sync::Arc::new(hook));
        self
    }

    /// Registers an ordinary HTTP route in this service.
    ///
    /// Creates a leaf [`Handler`] node for the route and records the route
    /// info for building the ordinary HTTP routing table.
    #[doc(hidden)]
    #[cfg(feature = "ordinary-http")]
    pub fn ordinary_route(
        mut self,
        method: &'static str,
        path: &'static str,
        entry: HandlerEntry,
    ) -> Self {
        let handler = Handler::ordinary_leaf(path, method, entry.clone());
        self.handlers.push(handler);
        self.ordinary_routes.push(OrdinaryRouteInfo {
            method,
            path: path.to_string(),
            handler_entry: entry,
            service_name: self.name.clone(),
        });
        self
    }

    /// Registers an ordinary WebSocket route in this service.
    #[doc(hidden)]
    #[cfg(feature = "ordinary-ws")]
    pub fn ws_route(
        mut self,
        path: &'static str,
        invoker: &'static dyn crate::app::ordinary_ws::WsHandlerInvoker,
        handler_name: &'static str,
    ) -> Self {
        self.ws_routes.push(crate::app::ordinary_ws::WsRouteInfo {
            path,
            handler_name,
            pattern: crate::app::ordinary::RoutePattern::parse(path),
            invoker,
            service_name: self.name.clone(),
            attrs: &[],
        });
        self
    }

    /// Registers an ordinary SSE route in this service.
    #[doc(hidden)]
    #[cfg(feature = "ordinary-sse")]
    pub fn sse_route(
        mut self,
        path: &'static str,
        invoker: &'static dyn crate::app::ordinary_sse::SseHandlerInvoker,
        handler_name: &'static str,
    ) -> Self {
        self.sse_routes
            .push(crate::app::ordinary_sse::SseRouteInfo {
                path,
                handler_name,
                pattern: crate::app::ordinary::RoutePattern::parse(path),
                invoker,
                service_name: self.name.clone(),
                attrs: &[],
            });
        self
    }
}

/// Counts the total number of handler nodes in a tree, including all nested
/// children. Used to compute the `offset` field for binary protocol dispatch.
pub fn count_handlers(handlers: &[Handler]) -> usize {
    let mut total = handlers.len();
    for h in handlers {
        total += count_handlers(&h.children);
    }
    total
}
