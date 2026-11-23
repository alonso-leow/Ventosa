# HubSpot
<!-- group: integrations -->

Sponsors and partners as a CRM: deals per sponsored video.

## 1. A private app token

HubSpot ▸ Settings ▸ **Integrations** ▸ **Private apps** ▸ **Create a private app** ▸ Scopes (e.g. `crm.objects.contacts.read`, `crm.objects.deals.read`, `crm.objects.deals.write`) ▸ Create ▸ copy the token (`pat-…`). Env `HUBSPOT_TOKEN`. (Newer accounts may list these under **Development ▸ Legacy apps**; the token is the same kind.)

![HubSpot, private app scopes](img/hubspot/private-app.png)

## 2. Calls

`https://api.hubapi.com/crm/v3/objects/deals` with `Authorization: Bearer <token>`.

## 3. In the pipeline

When a script has a sponsor beat, a step writes a note on the sponsor's deal with the video's link and publish date. Contract and price changes are elevate rules.
