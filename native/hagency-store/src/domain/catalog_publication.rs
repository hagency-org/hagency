//! Public catalog data, never a resource allocation or execution capability.
use super::*;
use crate::outbound::RegistrationIdentity;

const PER_ROLE: usize = 200;
// Leave room for the outer v2 generation/sequence fields. FreezePublication
// independently checks its exact final encoded body against the existing 1 MiB.
const MAX_BODY: usize = 1024 * 1024 - 512;

pub struct PublishedCatalog {
    body: Value,
}
impl PublishedCatalog {
    pub fn into_update(self) -> Value {
        self.body
    }
}

fn registration(db: &Connection, identity: &RegistrationIdentity) -> Result<Registration, Error> {
    project::identifier(&identity.fleet_id, 128)?;
    if identity.registration_fingerprint.len() != 64
        || !identity
            .registration_fingerprint
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::Generation);
    }
    let encoded: String = db
        .query_row(
            "SELECT config FROM registrations WHERE fleet_id=?1",
            [&identity.fleet_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    let current: Registration = serde_json::from_str(&encoded)?;
    current.validate()?;
    if current.fleet_id != identity.fleet_id
        || current.generation != identity.registration_generation
        || canonical::digest(&serde_json::to_value(&current)?)? != identity.registration_fingerprint
    {
        return Err(Error::Generation);
    }
    Ok(current)
}

fn label(resource: &CatalogResource) -> String {
    let full = format!("{} · {}", resource.framework, resource.model);
    if full.encode_utf16().count() <= 128 {
        return full;
    }
    let mut result = String::new();
    let mut units = 0;
    for c in full.chars() {
        if units + c.len_utf16() > 127 {
            break;
        }
        units += c.len_utf16();
        result.push(c);
    }
    result.push('…');
    result
}

fn public_resource(resource: &CatalogResource) -> Result<Value, Error> {
    if resource.framework.encode_utf16().count() > 32
        || resource.model.encode_utf16().count() > 256
        || resource
            .reasoning
            .as_ref()
            .is_some_and(|s| s.encode_utf16().count() > 64)
    {
        return Err(Error::Capacity);
    }
    Ok(json!({
        "id":resource.id, "name":label(resource), "framework":resource.framework,
        "model":resource.model, "reasoning":resource.reasoning,
    }))
}

impl DomainRepository {
    /// Current registration check independent of later resource/catalog changes.
    /// A pending frozen publication must not be replaced by a fresh snapshot.
    pub fn check_publication_registration(
        &self,
        identity: &RegistrationIdentity,
    ) -> Result<(), Error> {
        registration(&self.db, identity).map(|_| ())
    }

    /// One original writer operation supplies the whole current observation.
    /// No await, competing writer or second connection can mix its catalog pages.
    pub fn published_catalog(
        &self,
        identity: &RegistrationIdentity,
    ) -> Result<PublishedCatalog, Error> {
        let current = registration(&self.db, identity)?;
        let mut offers: Vec<(String, Vec<Value>)> = qualification::roles()
            .map(|role| (role.to_owned(), Vec::new()))
            .collect();
        let mut after = String::new();
        loop {
            let page = self.catalog_for(Some(&current.fleet_id), &after, 100)?;
            let finished = page.len() < 100;
            for resource in page {
                for (role, resources) in &mut offers {
                    if resource.roles.contains(role) {
                        if resources.len() == PER_ROLE {
                            return Err(Error::Capacity);
                        }
                        resources.push(public_resource(&resource)?);
                    }
                }
                after = resource.id;
            }
            if finished {
                break;
            }
        }
        let offers: Vec<Value> = offers
            .into_iter()
            .filter(|(_, resources)| !resources.is_empty())
            .map(|(role, resources)| json!({"role":role,"published":true,"resources":resources}))
            .collect();
        let body = json!({"heartbeat":true,"capabilities":{
            "v":1,"fleetId":current.fleet_id,"serverName":current.server_name,
            "representativeMxid":current.representative_mxid,
            "approvalBotMxid":current.approval_bot_mxid,"offers":offers,
        }});
        if canonical::encode_transport(&body)?.len() > MAX_BODY {
            return Err(Error::Capacity);
        }
        Ok(PublishedCatalog { body })
    }
}
