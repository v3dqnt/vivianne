import json
import re

ts_content = """
    "black-forest-labs/flux.2-flex": {
        id: "black-forest-labs/flux.2-flex",
        name: "Black Forest Labs: FLUX.2 Flex",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "black-forest-labs/flux.2-klein-4b": {
        id: "black-forest-labs/flux.2-klein-4b",
        name: "Black Forest Labs: FLUX.2 Klein 4B",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "black-forest-labs/flux.2-max": {
        id: "black-forest-labs/flux.2-max",
        name: "Black Forest Labs: FLUX.2 Max",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "black-forest-labs/flux.2-pro": {
        id: "black-forest-labs/flux.2-pro",
        name: "Black Forest Labs: FLUX.2 Pro",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "bytedance-seed/seedream-4.5": {
        id: "bytedance-seed/seedream-4.5",
        name: "ByteDance Seed: Seedream 4.5",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["image","text"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "google/gemini-2.5-flash-image": {
        id: "google/gemini-2.5-flash-image",
        name: "Google: Nano Banana (Gemini 2.5 Flash Image)",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["image","text"],
        output: ["image","text"],
        cost: 	{
  "input": 0.3,
  "output": 2.5,
  "cacheRead": 0.03,
  "cacheWrite": 0.08333333333333334
}
    } satisfies ImagesModel<"openrouter-images">,
    "google/gemini-3-pro-image": {
        id: "google/gemini-3-pro-image",
        name: "Google: Nano Banana Pro (Gemini 3 Pro Image)",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["image","text"],
        output: ["image","text"],
        cost: 	{
  "input": 2,
  "output": 12,
  "cacheRead": 0.19999999999999998,
  "cacheWrite": 0.375
}
    } satisfies ImagesModel<"openrouter-images">,
    "google/gemini-3-pro-image-preview": {
        id: "google/gemini-3-pro-image-preview",
        name: "Google: Nano Banana Pro (Gemini 3 Pro Image Preview)",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["image","text"],
        output: ["image","text"],
        cost: 	{
  "input": 2,
  "output": 12,
  "cacheRead": 0.19999999999999998,
  "cacheWrite": 0.375
}
    } satisfies ImagesModel<"openrouter-images">,
    "google/gemini-3.1-flash-image": {
        id: "google/gemini-3.1-flash-image",
        name: "Google: Nano Banana 2 (Gemini 3.1 Flash Image)",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["image","text"],
        output: ["image","text"],
        cost: 	{
  "input": 0.5,
  "output": 3,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "google/gemini-3.1-flash-image-preview": {
        id: "google/gemini-3.1-flash-image-preview",
        name: "Google: Nano Banana 2 (Gemini 3.1 Flash Image Preview)",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["image","text"],
        output: ["image","text"],
        cost: 	{
  "input": 0.5,
  "output": 3,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "google/gemini-3.1-flash-lite-image": {
        id: "google/gemini-3.1-flash-lite-image",
        name: "Google: Nano Banana 2 Lite (Gemini 3.1 Flash Lite Image)",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["image","text"],
        output: ["image","text"],
        cost: 	{
  "input": 0.25,
  "output": 1.5,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "microsoft/mai-image-2.5": {
        id: "microsoft/mai-image-2.5",
        name: "Microsoft: MAI-Image-2.5",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 5,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "openai/gpt-5-image": {
        id: "openai/gpt-5-image",
        name: "OpenAI: GPT-5 Image",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["image","text"],
        output: ["image","text"],
        cost: 	{
  "input": 10,
  "output": 10,
  "cacheRead": 1.25,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "openai/gpt-5-image-mini": {
        id: "openai/gpt-5-image-mini",
        name: "OpenAI: GPT-5 Image Mini",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["image","text"],
        output: ["image","text"],
        cost: 	{
  "input": 2.5,
  "output": 2,
  "cacheRead": 0.25,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "openai/gpt-5.4-image-2": {
        id: "openai/gpt-5.4-image-2",
        name: "OpenAI: GPT-5.4 Image 2",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["image","text"],
        output: ["image","text"],
        cost: 	{
  "input": 8,
  "output": 15,
  "cacheRead": 2,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "openai/gpt-image-1": {
        id: "openai/gpt-image-1",
        name: "OpenAI: GPT Image 1",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 10,
  "output": 10,
  "cacheRead": 1.25,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "openai/gpt-image-1-mini": {
        id: "openai/gpt-image-1-mini",
        name: "OpenAI: GPT Image 1 Mini",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 2.5,
  "output": 2.5,
  "cacheRead": 0.25,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "openai/gpt-image-2": {
        id: "openai/gpt-image-2",
        name: "OpenAI: GPT Image 2",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 8,
  "output": 8,
  "cacheRead": 2,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "openrouter/auto": {
        id: "openrouter/auto",
        name: "Auto Router",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["text","image"],
        cost: 	{
  "input": -1000000,
  "output": -1000000,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "recraft/recraft-v3": {
        id: "recraft/recraft-v3",
        name: "Recraft: Recraft V3",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "recraft/recraft-v4": {
        id: "recraft/recraft-v4",
        name: "Recraft: Recraft V4",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "recraft/recraft-v4-pro": {
        id: "recraft/recraft-v4-pro",
        name: "Recraft: Recraft V4 Pro",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "recraft/recraft-v4-pro-vector": {
        id: "recraft/recraft-v4-pro-vector",
        name: "Recraft: Recraft V4 Pro Vector",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "recraft/recraft-v4-vector": {
        id: "recraft/recraft-v4-vector",
        name: "Recraft: Recraft V4 Vector",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "recraft/recraft-v4.1": {
        id: "recraft/recraft-v4.1",
        name: "Recraft: Recraft V4.1",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "recraft/recraft-v4.1-pro": {
        id: "recraft/recraft-v4.1-pro",
        name: "Recraft: Recraft V4.1 Pro",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "recraft/recraft-v4.1-pro-vector": {
        id: "recraft/recraft-v4.1-pro-vector",
        name: "Recraft: Recraft V4.1 Pro Vector",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "recraft/recraft-v4.1-utility": {
        id: "recraft/recraft-v4.1-utility",
        name: "Recraft: Recraft V4.1 Utility",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "recraft/recraft-v4.1-utility-pro": {
        id: "recraft/recraft-v4.1-utility-pro",
        name: "Recraft: Recraft V4.1 Utility Pro",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "recraft/recraft-v4.1-vector": {
        id: "recraft/recraft-v4.1-vector",
        name: "Recraft: Recraft V4.1 Vector",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "sourceful/riverflow-v2-fast": {
        id: "sourceful/riverflow-v2-fast",
        name: "Sourceful: Riverflow V2 Fast",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "sourceful/riverflow-v2-pro": {
        id: "sourceful/riverflow-v2-pro",
        name: "Sourceful: Riverflow V2 Pro",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "sourceful/riverflow-v2.5-fast": {
        id: "sourceful/riverflow-v2.5-fast",
        name: "Sourceful: Riverflow V2.5 Fast",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "sourceful/riverflow-v2.5-pro": {
        id: "sourceful/riverflow-v2.5-pro",
        name: "Sourceful: Riverflow V2.5 Pro",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">,
    "x-ai/grok-imagine-image-quality": {
        id: "x-ai/grok-imagine-image-quality",
        name: "xAI: Grok Imagine Image Quality",
        api: "openrouter-images",
        provider: "openrouter",
        baseUrl: "https://openrouter.ai/api/v1",
        input: ["text","image"],
        output: ["image"],
        cost: 	{
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
    } satisfies ImagesModel<"openrouter-images">
"""

rust_code = """use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cost {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagesModel {
    pub id: String,
    pub name: String,
    pub api: String,
    pub provider: String,
    pub base_url: String,
    pub input: Vec<String>,
    pub output: Vec<String>,
    pub cost: Cost,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ImagesContent {
    Text { text: String },
    Image { data: String, mime_type: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagesContext {
    pub input: Vec<ImagesContent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantImages {
    pub api: String,
    pub provider: String,
    pub model: String,
    pub output: Vec<ImagesContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    pub stop_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImagesOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[async_trait]
pub trait ImagesApiProvider: Send + Sync {
    fn api(&self) -> &str;
    async fn generate_images(
        &self,
        model: &ImagesModel,
        context: &ImagesContext,
        options: Option<&ImagesOptions>,
    ) -> Result<AssistantImages, String>;
}

static PROVIDER_REGISTRY: OnceLock<RwLock<HashMap<String, Arc<dyn ImagesApiProvider>>>> = OnceLock::new();

pub fn register_images_api_provider(provider: Arc<dyn ImagesApiProvider>) {
    let registry = PROVIDER_REGISTRY.get_or_init(|| RwLock::new(HashMap::new()));
    registry.write().unwrap().insert(provider.api().to_string(), provider);
}

pub fn get_images_api_provider(api: &str) -> Option<Arc<dyn ImagesApiProvider>> {
    let registry = PROVIDER_REGISTRY.get_or_init(|| RwLock::new(HashMap::new()));
    registry.read().unwrap().get(api).cloned()
}

pub async fn generate_images(
    model: &ImagesModel,
    context: &ImagesContext,
    options: Option<&ImagesOptions>,
) -> Result<AssistantImages, String> {
    let provider = get_images_api_provider(&model.api)
        .ok_or_else(|| format!("No API provider registered for api: {}", model.api))?;
    
    provider.generate_images(model, context, options).await
}

pub fn get_image_models() -> &'static HashMap<String, ImagesModel> {
    static MODELS: OnceLock<HashMap<String, ImagesModel>> = OnceLock::new();
    MODELS.get_or_init(|| {
        let mut map = HashMap::new();
        
        let models_data = vec![
"""

pattern = re.compile(r"id:\s*\"(.*?)\",\s*name:\s*\"(.*?)\",\s*api:\s*\"(.*?)\",\s*provider:\s*\"(.*?)\",\s*baseUrl:\s*\"(.*?)\",\s*input:\s*\[(.*?)\],\s*output:\s*\[(.*?)\],\s*cost:\s*\{\s*\"input\":\s*([^,]+),\s*\"output\":\s*([^,]+),\s*\"cacheRead\":\s*([^,]+),\s*\"cacheWrite\":\s*([^\n}]+)\s*\}", re.DOTALL)

for match in pattern.finditer(ts_content):
    id_val = match.group(1)
    name = match.group(2)
    api = match.group(3)
    provider = match.group(4)
    base_url = match.group(5)
    
    in_list = [x.strip("\"'") for x in match.group(6).split(",")]
    out_list = [x.strip("\"'") for x in match.group(7).split(",")]
    
    in_arr = ", ".join(f"\"{x}\".to_string()" for x in in_list)
    out_arr = ", ".join(f"\"{x}\".to_string()" for x in out_list)
    
    input_cost = match.group(8).strip()
    output_cost = match.group(9).strip()
    cread_cost = match.group(10).strip()
    cwrite_cost = match.group(11).strip()
    
    if "." not in input_cost: input_cost += ".0"
    if "." not in output_cost: output_cost += ".0"
    if "." not in cread_cost: cread_cost += ".0"
    if "." not in cwrite_cost: cwrite_cost += ".0"
    
    rust_code += f"""            ImagesModel {{
                id: \"{id_val}\".to_string(),
                name: \"{name}\".to_string(),
                api: \"{api}\".to_string(),
                provider: \"{provider}\".to_string(),
                base_url: \"{base_url}\".to_string(),
                input: vec![{in_arr}],
                output: vec![{out_arr}],
                cost: Cost {{
                    input: {input_cost},
                    output: {output_cost},
                    cache_read: {cread_cost},
                    cache_write: {cwrite_cost},
                }},
            }},
"""

rust_code += """        ];

        for m in models_data {
            map.insert(m.id.clone(), m);
        }
        
        map
    })
}

pub fn get_image_model(id: &str) -> Option<ImagesModel> {
    get_image_models().get(id).cloned()
}

pub fn get_image_providers() -> Vec<String> {
    let mut providers = Vec::new();
    let models = get_image_models();
    for (_, model) in models.iter() {
        if !providers.contains(&model.provider) {
            providers.push(model.provider.clone());
        }
    }
    providers.sort();
    providers
}

pub fn get_image_models_by_provider(provider: &str) -> Vec<ImagesModel> {
    let mut result = Vec::new();
    for (_, model) in get_image_models().iter() {
        if model.provider == provider {
            result.push(model.clone());
        }
    }
    result
}
"""

with open("images.rs", "w", encoding="utf-8") as f:
    f.write(rust_code)
