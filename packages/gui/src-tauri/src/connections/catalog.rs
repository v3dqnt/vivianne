//! The apps Vivianne can connect to in one click.
//!
//! Every remote entry was checked against the live server: it speaks MCP
//! (Streamable HTTP), and its sign-in supports dynamic client registration,
//! so Vivianne can register herself and send the user to the app's own
//! approval page with no keys to copy. Where an app offers no such sign-in
//! (GitHub) the user pastes a token instead; where it has no hosted server at
//! all (Telegram, Slack bots, Discord, WhatsApp) a connector built into the
//! app talks to its API directly.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub secret: bool,
    pub placeholder: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    /// A hosted MCP server with self-service OAuth.
    OAuth { url: &'static str },
    /// A hosted MCP server that needs no sign-in.
    Open { url: &'static str },
    /// A hosted MCP server that takes a pasted token in a header.
    Token { url: &'static str, header: &'static str, prefix: &'static str },
    /// Built into the app: talks to the service's API directly.
    Native,
}

#[derive(Debug, Clone, Copy)]
pub struct App {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    /// For the icon.
    pub domain: &'static str,
    pub kind: Kind,
    /// What to paste, for Token and Native apps.
    pub fields: &'static [Field],
    /// How to get it, in a sentence, and where.
    pub help: &'static str,
    pub help_url: &'static str,
}

const fn oauth(id: &'static str, name: &'static str, category: &'static str, description: &'static str, domain: &'static str, url: &'static str) -> App {
    App { id, name, category, description, domain, kind: Kind::OAuth { url }, fields: &[], help: "", help_url: "" }
}

const fn open(id: &'static str, name: &'static str, category: &'static str, description: &'static str, domain: &'static str, url: &'static str) -> App {
    App { id, name, category, description, domain, kind: Kind::Open { url }, fields: &[], help: "", help_url: "" }
}

const TOKEN: &[Field] = &[Field { key: "token", label: "Token", secret: true, placeholder: "" }];

pub const CATALOG: &[App] = &[
    // ── Communication ──
    App {
        id: "slack",
        name: "Slack",
        category: "Communication",
        description: "Read channels and send messages as your Slack bot.",
        domain: "slack.com",
        kind: Kind::Native,
        fields: &[Field { key: "token", label: "Bot token", secret: true, placeholder: "xoxb-…" }],
        help: "Create a Slack app, add the bot scopes channels:read, channels:history, groups:read, groups:history and chat:write, install it to your workspace, and paste its Bot User OAuth Token. Invite the bot to the channels it should see.",
        help_url: "https://api.slack.com/apps",
    },
    App {
        id: "telegram",
        name: "Telegram",
        category: "Communication",
        description: "Send and read messages through your own Telegram bot.",
        domain: "telegram.org",
        kind: Kind::Native,
        fields: &[Field { key: "token", label: "Bot token", secret: true, placeholder: "123456:ABC-…" }],
        help: "Message @BotFather on Telegram, send /newbot, and paste the token it gives you. People (and groups) need to message the bot once before it can write to them.",
        help_url: "https://t.me/BotFather",
    },
    App {
        id: "discord",
        name: "Discord",
        category: "Communication",
        description: "Read channels and post messages with your Discord bot.",
        domain: "discord.com",
        kind: Kind::Native,
        fields: &[Field { key: "token", label: "Bot token", secret: true, placeholder: "" }],
        help: "In the Discord Developer Portal create an application, add a bot, turn on the Message Content intent, copy its token, and invite it to your server with permission to read and send messages.",
        help_url: "https://discord.com/developers/applications",
    },
    App {
        id: "whatsapp",
        name: "WhatsApp",
        category: "Communication",
        description: "Send WhatsApp messages from your business number (WhatsApp Cloud API).",
        domain: "whatsapp.com",
        kind: Kind::Native,
        fields: &[
            Field { key: "token", label: "Access token", secret: true, placeholder: "EAAG…" },
            Field { key: "phone_number_id", label: "Phone number ID", secret: false, placeholder: "1098…" },
        ],
        help: "In Meta for Developers, create a Business app with WhatsApp, then copy a (permanent system user) access token and the phone number ID from WhatsApp → API Setup. Messages outside a 24-hour reply window must use an approved template.",
        help_url: "https://developers.facebook.com/apps",
    },
    oauth("intercom", "Intercom", "Sales & support", "Conversations, contacts and help articles.", "intercom.com", "https://mcp.intercom.com/mcp"),
    // ── Work ──
    oauth("notion", "Notion", "Work", "Search, read and write your Notion pages and databases.", "notion.so", "https://mcp.notion.com/mcp"),
    oauth("atlassian", "Jira & Confluence", "Work", "Issues, projects and Confluence pages.", "atlassian.com", "https://mcp.atlassian.com/v1/mcp"),
    oauth("linear", "Linear", "Work", "Issues, projects and cycles.", "linear.app", "https://mcp.linear.app/mcp"),
    oauth("monday", "monday.com", "Work", "Boards, items and updates.", "monday.com", "https://mcp.monday.com/mcp"),
    oauth("clickup", "ClickUp", "Work", "Tasks, lists and docs.", "clickup.com", "https://mcp.clickup.com/mcp"),
    oauth("todoist", "Todoist", "Work", "Your tasks and projects.", "todoist.com", "https://ai.todoist.net/mcp"),
    oauth("airtable", "Airtable", "Work", "Bases, tables and records.", "airtable.com", "https://mcp.airtable.com/mcp"),
    oauth("miro", "Miro", "Work", "Boards: read them and add to them.", "miro.com", "https://mcp.miro.com/mcp"),
    oauth("dropbox", "Dropbox", "Work", "Find, read and save files.", "dropbox.com", "https://mcp.dropbox.com/mcp"),
    oauth("egnyte", "Egnyte", "Work", "Company files and folders.", "egnyte.com", "https://mcp-server.egnyte.com/mcp"),
    oauth("calendly", "Calendly", "Work", "Event types, bookings and availability.", "calendly.com", "https://mcp.calendly.com/mcp"),
    oauth("cal", "Cal.com", "Work", "Bookings and availability.", "cal.com", "https://mcp.cal.com/mcp"),
    oauth("fireflies", "Fireflies", "Work", "Meeting transcripts and summaries.", "fireflies.ai", "https://api.fireflies.ai/mcp"),
    oauth("granola", "Granola", "Work", "Your meeting notes.", "granola.ai", "https://mcp.granola.ai/mcp"),
    oauth("zapier", "Zapier", "Work", "Thousands of apps through your Zapier actions.", "zapier.com", "https://mcp.zapier.com/api/mcp/mcp"),
    oauth("make", "Make", "Work", "Run your Make scenarios.", "make.com", "https://mcp.make.com"),
    oauth("asana", "Asana", "Work", "Tasks, projects and goals.", "asana.com", "https://mcp.asana.com/mcp"),
    oauth("basecamp", "Basecamp", "Work", "Projects, to-dos and message boards.", "basecamp.com", "https://mcp.basecamp.com/mcp"),
    oauth("shortcut", "Shortcut", "Work", "Stories, epics and iterations.", "shortcut.com", "https://mcp.shortcut.com/mcp"),
    oauth("productboard", "Productboard", "Work", "Product feedback, features and roadmaps.", "productboard.com", "https://mcp.productboard.com/mcp"),
    oauth("aha", "Aha!", "Work", "Product roadmaps, ideas and features.", "aha.io", "https://mcp.aha.io/mcp"),
    oauth("ticktick", "TickTick", "Work", "Your tasks and lists.", "ticktick.com", "https://mcp.ticktick.com/mcp"),
    oauth("anydo", "Any.do", "Work", "Your tasks, lists and reminders.", "any.do", "https://mcp.any.do/mcp"),
    oauth("reclaim", "Reclaim", "Work", "Smart scheduling: tasks, habits and focus time.", "reclaim.ai", "https://mcp.reclaim.ai/mcp"),
    oauth("harvest", "Harvest", "Work", "Time tracking, projects and invoices.", "harvestapp.com", "https://mcp.harvestapp.com/mcp"),
    oauth("slite", "Slite", "Work", "Your team's docs and knowledge base.", "slite.com", "https://mcp.slite.com/mcp"),
    oauth("hackmd", "HackMD", "Work", "Collaborative Markdown notes.", "hackmd.io", "https://mcp.hackmd.io/mcp"),
    oauth("docsend", "DocSend", "Work", "Documents, spaces and viewer analytics.", "docsend.com", "https://mcp.docsend.com/mcp"),
    oauth("pandadoc", "PandaDoc", "Work", "Proposals, quotes and contracts to sign.", "pandadoc.com", "https://mcp.pandadoc.com/mcp"),
    oauth("jotform", "Jotform", "Work", "Forms and their submissions.", "jotform.com", "https://mcp.jotform.com/mcp"),
    oauth("tally", "Tally", "Work", "Forms and their responses.", "tally.so", "https://mcp.tally.so/mcp"),
    oauth("otter", "Otter", "Work", "Meeting transcripts and summaries.", "otter.ai", "https://mcp.otter.ai/mcp"),
    oauth("tldv", "tl;dv", "Work", "Meeting recordings, notes and highlights.", "tldv.io", "https://mcp.tldv.io/mcp"),
    oauth("avoma", "Avoma", "Work", "Meeting notes, recordings and insights.", "avoma.com", "https://mcp.avoma.com/mcp"),
    // ── Finance, sales and support ──
    oauth("stripe", "Stripe", "Finance", "Customers, payments, invoices and products.", "stripe.com", "https://mcp.stripe.com"),
    oauth("paypal", "PayPal", "Finance", "Invoices, orders and transactions.", "paypal.com", "https://mcp.paypal.com/mcp"),
    oauth("square", "Square", "Finance", "Payments, orders, catalog and customers.", "squareup.com", "https://mcp.squareup.com/mcp"),
    oauth("plaid", "Plaid", "Finance", "Your Plaid dashboard: usage and items.", "plaid.com", "https://api.dashboard.plaid.com/mcp"),
    oauth("attio", "Attio", "Sales & support", "CRM records, lists and notes.", "attio.com", "https://mcp.attio.com/mcp"),
    oauth("close", "Close", "Sales & support", "Leads, contacts and activities.", "close.com", "https://mcp.close.com/mcp"),
    oauth("quickbooks", "QuickBooks", "Finance", "Reports, transactions and company books.", "quickbooks.intuit.com", "https://mcp.quickbooks.intuit.com/mcp"),
    oauth("ramp", "Ramp", "Finance", "Cards, spend, bills and reimbursements.", "ramp.com", "https://mcp.ramp.com/mcp"),
    oauth("mercury", "Mercury", "Finance", "Bank accounts and transactions.", "mercury.com", "https://mcp.mercury.com/mcp"),
    oauth("paddle", "Paddle", "Finance", "Products, subscriptions and transactions.", "paddle.com", "https://mcp.paddle.com/mcp"),
    oauth("polar", "Polar", "Finance", "Products, orders and subscribers.", "polar.sh", "https://mcp.polar.sh/mcp/polar-mcp"),
    open("coingecko", "CoinGecko", "Finance", "Crypto prices and market data.", "coingecko.com", "https://mcp.api.coingecko.com/mcp"),
    oauth("apollo", "Apollo", "Sales & support", "Find and enrich people and companies.", "apollo.io", "https://mcp.apollo.io/mcp"),
    oauth("gong", "Gong", "Sales & support", "Sales calls, deals and insights.", "gong.io", "https://mcp.gong.io/mcp"),
    oauth("klaviyo", "Klaviyo", "Marketing", "Email and SMS campaigns, flows and lists.", "klaviyo.com", "https://mcp.klaviyo.com/mcp"),
    oauth("customerio", "Customer.io", "Marketing", "Customer messaging: campaigns and segments.", "customer.io", "https://mcp.customer.io/mcp"),
    oauth("buffer", "Buffer", "Marketing", "Plan and schedule social posts.", "buffer.com", "https://mcp.buffer.com/mcp"),
    oauth("hootsuite", "Hootsuite", "Marketing", "Social posts, schedules and analytics.", "hootsuite.com", "https://mcp.hootsuite.com/mcp"),
    oauth("sproutsocial", "Sprout Social", "Marketing", "Social profiles, posts and reports.", "sproutsocial.com", "https://mcp.sproutsocial.com/mcp"),
    oauth("semrush", "Semrush", "Marketing", "SEO, keywords and competitor research.", "semrush.com", "https://mcp.semrush.com/v1/mcp"),
    oauth("ahrefs", "Ahrefs", "Marketing", "Backlinks, keywords and site audits.", "ahrefs.com", "https://api.ahrefs.com/mcp/mcp"),
    oauth("amplitude", "Amplitude", "Marketing", "Product analytics: charts, cohorts and experiments.", "amplitude.com", "https://mcp.amplitude.com/mcp"),
    oauth("mixpanel", "Mixpanel", "Marketing", "Product analytics: events, funnels and retention.", "mixpanel.com", "https://mcp.mixpanel.com/mcp"),
    oauth("greenhouse", "Greenhouse", "Hiring", "Jobs, candidates and interviews.", "greenhouse.io", "https://mcp.greenhouse.io/mcp"),
    oauth("ashby", "Ashby", "Hiring", "Jobs, candidates and pipelines.", "ashbyhq.com", "https://mcp.ashbyhq.com/mcp"),
    oauth("workable", "Workable", "Hiring", "Jobs, candidates and offers.", "workable.com", "https://mcp.workable.com/mcp"),
    oauth("teamtailor", "Teamtailor", "Hiring", "Jobs, candidates and career site.", "teamtailor.com", "https://mcp.teamtailor.com/mcp"),
    oauth("indeed", "Indeed", "Hiring", "Search jobs and companies.", "indeed.com", "https://mcp.indeed.com/claude/mcp"),
    open("dice", "Dice", "Hiring", "Search tech jobs.", "dice.com", "https://mcp.dice.com/mcp"),
    open("kiwi", "Kiwi.com", "Travel", "Search flights and fares.", "kiwi.com", "https://mcp.kiwi.com"),
    // ── Design & media ──
    oauth("canva", "Canva", "Design & media", "Find, create and edit designs.", "canva.com", "https://mcp.canva.com/mcp"),
    oauth("figma", "Figma", "Design & media", "Design files, components and variables.", "figma.com", "https://mcp.figma.com/mcp"),
    oauth("webflow", "Webflow", "Design & media", "Sites, pages and CMS collections.", "webflow.com", "https://mcp.webflow.com/mcp"),
    oauth("wix", "Wix", "Design & media", "Your Wix sites and business tools.", "wix.com", "https://mcp.wix.com/mcp"),
    oauth("gamma", "Gamma", "Design & media", "Generate decks, documents and sites.", "gamma.app", "https://mcp.gamma.app/mcp"),
    oauth("pitch", "Pitch", "Design & media", "Presentations.", "pitch.com", "https://mcp.pitch.com/mcp"),
    oauth("framer", "Framer", "Design & media", "Your Framer projects and sites.", "framer.com", "https://mcp.framer.com/mcp"),
    oauth("lucid", "Lucid", "Design & media", "Lucidchart and Lucidspark diagrams.", "lucid.app", "https://mcp.lucid.app/mcp"),
    oauth("midjourney", "Midjourney", "Design & media", "Create images with Midjourney.", "midjourney.com", "https://mcp.midjourney.com/mcp"),
    oauth("ideogram", "Ideogram", "Design & media", "Images with great typography.", "ideogram.ai", "https://mcp.ideogram.ai/mcp"),
    oauth("runway", "Runway", "Design & media", "Generate and edit video.", "runwayml.com", "https://mcp.runwayml.com/mcp"),
    oauth("pika", "Pika", "Design & media", "Generate video.", "pika.art", "https://mcp.pika.art/mcp"),
    oauth("heygen", "HeyGen", "Design & media", "AI avatar videos.", "heygen.com", "https://mcp.heygen.com/mcp"),
    oauth("invideo", "invideo", "Design & media", "Make videos from a prompt.", "invideo.io", "https://mcp.invideo.io/mcp"),
    oauth("fal", "fal", "Design & media", "Image, video and audio models.", "fal.ai", "https://mcp.fal.ai/mcp"),
    oauth("replicate", "Replicate", "Design & media", "Run thousands of open models.", "replicate.com", "https://mcp.replicate.com/mcp"),
    oauth("vimeo", "Vimeo", "Design & media", "Your videos, folders and stats.", "vimeo.com", "https://mcp.vimeo.com/mcp"),
    oauth("wistia", "Wistia", "Design & media", "Your videos, projects and stats.", "wistia.com", "https://mcp.wistia.com/mcp"),
    oauth("cloudinary", "Cloudinary", "Design & media", "Upload, find and transform media.", "cloudinary.com", "https://asset-management.mcp.cloudinary.com/mcp"),
    // ── Developer ──
    App {
        id: "github",
        name: "GitHub",
        category: "Developer",
        description: "Repositories, issues, pull requests and Actions.",
        domain: "github.com",
        kind: Kind::Token { url: "https://api.githubcopilot.com/mcp/", header: "Authorization", prefix: "Bearer " },
        fields: TOKEN,
        help: "Create a fine-grained personal access token with access to the repositories Vivianne should work with (Contents, Issues, Pull requests: read and write) and paste it here.",
        help_url: "https://github.com/settings/personal-access-tokens/new",
    },
    oauth("gitlab", "GitLab", "Developer", "Projects, issues and merge requests.", "gitlab.com", "https://gitlab.com/api/v4/mcp"),
    oauth("sentry", "Sentry", "Developer", "Errors, issues and performance.", "sentry.io", "https://mcp.sentry.dev/mcp"),
    oauth("vercel", "Vercel", "Developer", "Projects, deployments and logs.", "vercel.com", "https://mcp.vercel.com"),
    oauth("netlify", "Netlify", "Developer", "Sites, deploys and forms.", "netlify.com", "https://netlify-mcp.netlify.app/mcp"),
    oauth("supabase", "Supabase", "Developer", "Projects, databases and edge functions.", "supabase.com", "https://mcp.supabase.com/mcp"),
    oauth("neon", "Neon", "Developer", "Serverless Postgres projects and branches.", "neon.tech", "https://mcp.neon.tech/mcp"),
    oauth("cloudflare", "Cloudflare", "Developer", "Workers, KV, R2 and D1.", "cloudflare.com", "https://bindings.mcp.cloudflare.com/mcp"),
    oauth("posthog", "PostHog", "Developer", "Product analytics, flags and experiments.", "posthog.com", "https://mcp.posthog.com/mcp"),
    oauth("semgrep", "Semgrep", "Developer", "Code scanning and findings.", "semgrep.dev", "https://mcp.semgrep.ai/mcp"),
    oauth("jam", "Jam", "Developer", "Bug reports with recordings and logs.", "jam.dev", "https://mcp.jam.dev/mcp"),
    oauth("datadog", "Datadog", "Developer", "Logs, metrics, traces and monitors.", "datadoghq.com", "https://mcp.datadoghq.com/api/unstable/mcp-server/mcp"),
    oauth("grafana", "Grafana", "Developer", "Dashboards, queries and alerts.", "grafana.com", "https://mcp.grafana.com/mcp"),
    oauth("newrelic", "New Relic", "Developer", "Observability data and alerts.", "newrelic.com", "https://mcp.newrelic.com/mcp"),
    oauth("honeycomb", "Honeycomb", "Developer", "Traces, queries and SLOs.", "honeycomb.io", "https://mcp.honeycomb.io/mcp"),
    oauth("axiom", "Axiom", "Developer", "Logs and events: query them.", "axiom.co", "https://mcp.axiom.co/mcp"),
    oauth("betterstack", "Better Stack", "Developer", "Uptime, logs and incidents.", "betterstack.com", "https://mcp.betterstack.com/mcp"),
    oauth("incident", "incident.io", "Developer", "Incidents, alerts and on-call.", "incident.io", "https://mcp.incident.io/mcp"),
    oauth("circleci", "CircleCI", "Developer", "Pipelines, builds and test results.", "circleci.com", "https://mcp.circleci.com/mcp"),
    oauth("buildkite", "Buildkite", "Developer", "Pipelines, builds and jobs.", "buildkite.com", "https://mcp.buildkite.com/mcp"),
    oauth("heroku", "Heroku", "Developer", "Apps, dynos, add-ons and logs.", "heroku.com", "https://mcp.heroku.com/mcp"),
    oauth("railway", "Railway", "Developer", "Projects, services and deployments.", "railway.com", "https://mcp.railway.com/mcp"),
    oauth("upstash", "Upstash", "Developer", "Serverless Redis, Vector and QStash.", "upstash.com", "https://mcp.upstash.com/mcp"),
    oauth("prisma", "Prisma", "Developer", "Prisma Postgres databases.", "prisma.io", "https://mcp.prisma.io/mcp"),
    oauth("sanity", "Sanity", "Developer", "Content, schemas and datasets.", "sanity.io", "https://mcp.sanity.io/mcp"),
    oauth("contentful", "Contentful", "Developer", "Content types, entries and assets.", "contentful.com", "https://mcp.contentful.com/mcp"),
    oauth("storyblok", "Storyblok", "Developer", "Stories, components and assets.", "storyblok.com", "https://mcp.storyblok.com"),
    oauth("mintlify", "Mintlify", "Developer", "Your documentation site.", "mintlify.com", "https://mcp.mintlify.com/mcp"),
    oauth("postman", "Postman", "Developer", "Collections, APIs and environments.", "postman.com", "https://mcp.postman.com/mcp"),
    oauth("expo", "Expo", "Developer", "Builds, updates and app store submissions.", "expo.dev", "https://mcp.expo.dev/mcp"),
    oauth("lovable", "Lovable", "Developer", "Build and edit your Lovable apps.", "lovable.dev", "https://mcp.lovable.dev/mcp"),
    oauth("stytch", "Stytch", "Developer", "Auth projects and settings.", "stytch.com", "https://mcp.stytch.dev/mcp"),
    oauth("algolia", "Algolia", "Developer", "Search indices, records and analytics.", "algolia.com", "https://mcp.algolia.com/mcp"),
    oauth("browserstack", "BrowserStack", "Developer", "Test on real browsers and devices.", "browserstack.com", "https://mcp.browserstack.com/mcp"),
    oauth("apify", "Apify", "Developer", "Thousands of ready-made scrapers and actors.", "apify.com", "https://mcp.apify.com/mcp"),
    oauth("mapbox", "Mapbox", "Developer", "Maps, styles, geocoding and directions.", "mapbox.com", "https://mcp.mapbox.com/mcp"),
    oauth("mux", "Mux", "Developer", "Video streaming: assets and live streams.", "mux.com", "https://mcp.mux.com/mcp"),
    open("socket", "Socket", "Developer", "Check dependencies for supply-chain risk.", "socket.dev", "https://mcp.socket.dev"),
    // ── Knowledge ──
    open("context7", "Context7", "Knowledge", "Up-to-date docs and examples for libraries.", "context7.com", "https://mcp.context7.com/mcp"),
    open("deepwiki", "DeepWiki", "Knowledge", "Ask questions about any public GitHub repository.", "deepwiki.com", "https://mcp.deepwiki.com/mcp"),
    open("mslearn", "Microsoft Learn", "Knowledge", "Microsoft and Azure documentation.", "learn.microsoft.com", "https://learn.microsoft.com/api/mcp"),
    open("cloudflare_docs", "Cloudflare Docs", "Knowledge", "Cloudflare's documentation.", "developers.cloudflare.com", "https://docs.mcp.cloudflare.com/mcp"),
    open("huggingface", "Hugging Face", "Knowledge", "Models, datasets, Spaces and papers.", "huggingface.co", "https://huggingface.co/mcp"),
    open("exa", "Exa", "Knowledge", "Web search built for AI.", "exa.ai", "https://mcp.exa.ai/mcp"),
    oauth("gitbook", "GitBook", "Knowledge", "Your GitBook spaces and docs.", "gitbook.com", "https://mcp.gitbook.com/mcp"),
    oauth("readwise", "Readwise", "Knowledge", "Your highlights and saved articles.", "readwise.io", "https://mcp.readwise.io/mcp"),
    oauth("tavily", "Tavily", "Knowledge", "Web search and extraction for AI.", "tavily.com", "https://mcp.tavily.com/mcp"),
    open("aws_docs", "AWS Docs", "Knowledge", "AWS documentation and regional availability.", "aws.amazon.com", "https://knowledge-mcp.global.api.aws"),
    open("svelte", "Svelte", "Knowledge", "Svelte and SvelteKit docs, plus an autofixer.", "svelte.dev", "https://mcp.svelte.dev/mcp"),
    open("grep", "grep.app", "Knowledge", "Search code across a million GitHub repos.", "grep.app", "https://mcp.grep.app"),
];

pub fn find(id: &str) -> Option<&'static App> {
    CATALOG.iter().find(|a| a.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_tool_safe() {
        let mut seen = std::collections::HashSet::new();
        for a in CATALOG {
            assert!(seen.insert(a.id), "duplicate {}", a.id);
            assert!(a.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'), "{}", a.id);
            if matches!(a.kind, Kind::Native | Kind::Token { .. }) {
                assert!(!a.fields.is_empty() && !a.help.is_empty() && a.help_url.starts_with("https://"), "{}", a.id);
            }
        }
    }
    /// Every app that signs in publishes its sign-in details and lets an app
    /// register itself. Read-only: discovery, no registration.
    #[tokio::test]
    #[ignore = "network: run manually"]
    async fn every_sign_in_app_is_discoverable() {
        let checks = CATALOG.iter().filter_map(|a| match a.kind {
            Kind::OAuth { url } => Some(async move { (a.id, super::super::oauth::can_register(url).await) }),
            _ => None,
        });
        use futures::StreamExt;
        let mut results: Vec<_> = futures::stream::iter(checks).buffer_unordered(8).collect().await;
        // A slow server under load isn't a broken one: retry those once, alone.
        for (id, r) in results.iter_mut() {
            if !matches!(r, Ok(true)) {
                if let Some(Kind::OAuth { url }) = find(id).map(|a| &a.kind) {
                    *r = super::super::oauth::can_register(url).await;
                }
            }
        }
        let failed: Vec<String> = results
            .into_iter()
            .filter_map(|(id, r)| match r {
                Ok(true) => None,
                Ok(false) => Some(format!("{id}: no registration endpoint")),
                Err(e) => Some(format!("{id}: {e}")),
            })
            .collect();
        assert!(failed.is_empty(), "{failed:#?}");
    }
}
