# Mailchimp
<!-- group: integrations -->

Newsletters for each release: the AI drafts, a person sends.

## 1. API key

Profile ▸ **Extras** ▸ **API keys** ▸ **Create a key**. The key ends with the data centre, e.g. `…-us21`; the API base is `https://us21.api.mailchimp.com/3.0`. Put the key in `MAILCHIMP_API_KEY`.

![Mailchimp, API keys under Extras](img/mailchimp/api-keys.png)

## 2. Audience id

Audience ▸ **All contacts** ▸ **Settings** ▸ **Audience name and defaults**: the Audience ID.

## 3. In the pipeline

A step after `publish`: owner AI, need "a campaign draft for the release", write = a custom tool that creates the campaign (`POST /campaigns`, then `PUT /campaigns/{id}/content`). Sending stays a human step (elevate rule: "sending to the list").
