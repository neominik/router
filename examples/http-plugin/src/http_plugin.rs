use std::ops::ControlFlow;
use std::sync::Arc;

use apollo_router::graphql;
use apollo_router::layers::ServiceBuilderExt;
use apollo_router::plugin::Plugin;
use apollo_router::plugin::PluginInit;
use apollo_router::register_plugin;
use apollo_router::services::execution;
use apollo_router::services::subgraph;
use apollo_router::services::supergraph;
use apollo_router::services::utility;
use apollo_router::Context;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use tower::BoxError;
use tower::ServiceBuilder;
use tower::ServiceExt;

#[derive(Debug)]
struct HttpPlugin {
    configuration: Conf,
    sdl: Arc<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
struct Conf {
    // Put your plugin configuration here. It will automatically be deserialized from JSON.
    url: String, // The url you'd like to offload processing to
}

#[derive(Debug, Deserialize, Serialize)]
struct Output {
    context: Context,
    sdl: Arc<String>,
    body: graphql::Request,
}

// This is a bare bones plugin that can be duplicated when creating your own.
#[async_trait::async_trait]
impl Plugin for HttpPlugin {
    type Config = Conf;

    async fn new(init: PluginInit<Self::Config>) -> Result<Self, BoxError> {
        Ok(HttpPlugin {
            configuration: init.config,
            sdl: init.supergraph_sdl,
        })
    }

    fn supergraph_service(&self, service: supergraph::BoxService) -> supergraph::BoxService {
        // Always use service builder to compose your plugins.
        // It provides off the shelf building blocks for your plugin.
        let proto_url = self.configuration.url.clone();
        let sdl = self.sdl.clone();
        ServiceBuilder::new()
            .checkpoint_async(move |mut request: supergraph::Request| {
                let proto_url = proto_url.clone();
                let my_sdl = sdl.to_string();

                async move {
                    // Call into our out of process processor with a body of our body

                    let my_request = request.supergraph_request.body().clone();
                    let my_context = request.context.clone();
                    let modified_output =
                        utility::call_with_request(&proto_url, my_request, my_context, my_sdl)
                            .await
                            .map_err(|e: BoxError| e.to_string())?;
                    // tracing::info!("modified output: {:?}", modified_output);
                    *request.supergraph_request.body_mut() = modified_output.body;
                    request.context = modified_output.context;

                    // Figure out a way to allow our external processor to interact with
                    // headers and extensions. Probably don't want to allow other things
                    // to be changed (version, etc...)
                    // None of these things can be serialized just now.
                    /*
                    let hdrs = serde_json::to_string(&request.supergraph_request.headers())?;
                    let extensions =
                        serde_json::to_string(&request.supergraph_request.extensions())?;
                    */
                    Ok(ControlFlow::Continue(request))
                }
            })
            // .map_response()
            // .rate_limit()
            // .checkpoint()
            // .timeout()
            .buffer(20_000)
            .service(service)
            .boxed()
    }

    fn execution_service(&self, service: execution::BoxService) -> execution::BoxService {
        //This is the default implementation and does not modify the default service.
        // The trait also has this implementation, and we just provide it here for illustration.
        service
    }

    // Called for each subgraph
    fn subgraph_service(&self, _name: &str, service: subgraph::BoxService) -> subgraph::BoxService {
        // Always use service builder to compose your plugins.
        // It provides off the shelf building blocks for your plugin.
        ServiceBuilder::new()
            // .map_request()
            // .map_response()
            // .rate_limit()
            // .checkpoint()
            // .timeout()
            .service(service)
            .boxed()
    }
}

// This macro allows us to use it in our plugin registry!
// register_plugin takes a group name, and a plugin name.
//
// In order to keep the plugin names consistent,
// we use using the `Reverse domain name notation`
register_plugin!("example", "http_plugin", HttpPlugin);

#[cfg(test)]
mod tests {
    // If we run this test as follows: cargo test -- --nocapture
    // we will see the message "Hello Bob" printed to standard out
    #[tokio::test]
    async fn display_message() {
        let config = serde_json::json!({
            "plugins": {
                "example.http_plugin": {
                    "url": "http://127.0.0.1:8081"
                }
            }
        });
        // Build a test harness. Usually we'd use this and send requests to
        // it, but in this case it's enough to build the harness to see our
        // output when our service registers.
        let _test_harness = apollo_router::TestHarness::builder()
            .configuration_json(config)
            .unwrap()
            .build()
            .await
            .unwrap();
    }
}
