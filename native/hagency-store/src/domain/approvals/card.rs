//! Private metadata only. A packet is neither send nor owner-decision authority.
use super::*;

const MAX_CARD: usize = 48 * 1024;

pub struct PrivateApprovalCard {
    target: ApprovalIntakeTarget,
    owner_expires_at: u64,
    content: Value,
}
impl PrivateApprovalCard {
    pub fn target(&self) -> &ApprovalIntakeTarget {
        &self.target
    }
    pub fn owner_expires_at(&self) -> u64 {
        self.owner_expires_at
    }
    /// Secret-bearing owner message. Never project into the public console.
    pub fn content(&self) -> &Value {
        &self.content
    }
}

impl DomainRepository {
    pub fn private_approval_card(
        &mut self,
        id: &str,
        owner_expires_at: u64,
        now: u64,
    ) -> Result<PrivateApprovalCard, Error> {
        self.private_approval_card_clock(id, owner_expires_at, || Ok(now))
    }

    pub(crate) fn private_approval_card_clock(
        &mut self,
        id: &str,
        owner_expires_at: u64,
        sample: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<PrivateApprovalCard, Error> {
        identifier(id, 128)?;
        clock(owner_expires_at)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = sample()?;
        clock(now)?;
        let target = intake_target(&tx, id, now)?;
        if owner_expires_at <= now || owner_expires_at > target.expires_at {
            return Err(Error::RunnerAuthority);
        }
        let (context, request) = request(&tx, id)?;
        let (description, scope_kind): (Option<String>, Option<String>) = tx.query_row(
            "SELECT description,scope_kind FROM owner_approvals WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut description = description.unwrap_or_default();
        let mut actions =
            vec![json!({"id":"approve_once","label":"Approve once","style":"primary"})];
        let scope = if target.reusable_scope {
            // Stored scope is the one already derived at request admission.
            // Formatting never recalculates or broadens its authorization.
            if description.is_empty() {
                return Err(Error::Schema);
            }
            let kind: Value = serde_json::from_str(scope_kind.as_deref().ok_or(Error::Schema)?)?;
            let scope = json!({"kind":kind,"description":description,
                "workspace":context.workspace,"task_id":context.task});
            description.push_str(&format!(
                "\nAuthorization scope for {} in project {}:\nWorkspace: {}\nThis task: {}\nAlways allow saves this exact rule for this Agent and project.",
                target.authority.engagement_id, target.authority.project_id,
                context.workspace, context.task
            ));
            actions.push(
                json!({"id":"approve_task","label":"Allow for this task","style":"secondary"}),
            );
            actions.push(json!({"id":"approve_always","label":"Always allow this operation","style":"secondary"}));
            Some(scope)
        } else {
            None
        };
        actions.push(json!({"id":"deny","label":"Deny","style":"danger"}));
        let preview = serde_json::to_string(&request.params)?;
        // The retained v1 client expects a nonempty display string. Preserve
        // the actual JSON-RPC type separately; neither field is authority.
        let upstream_display = match &request.upstream {
            ApprovalRpcId::Number(value) => value.to_string(),
            ApprovalRpcId::String(value) => value.clone(),
        };
        let mut detail = json!({
            "version":1,"kind":"request",
            "agent":target.authority.engagement_id,
            "project":target.authority.project_id,
            "project_room_id":target.authority.project_room_id,
            "request_id":target.request_id,"input_digest":target.request_digest,
            "upstream_request_id":upstream_display,"upstream_rpc_id":request.upstream,"runtime":"codex",
            "tool_name":request.method,"description":description,
            "input_preview":preview,"expires_at":owner_expires_at,"actions":actions
        });
        if let Some(scope) = scope {
            detail["reusable_scope"] = scope;
        }
        let expires = time::OffsetDateTime::from_unix_timestamp_nanos(
            i128::from(owner_expires_at) * 1_000_000,
        )
        .map_err(|_| Error::RunnerAuthority)?;
        let body = format!(
            "Approval required for {}\nProject: {}\nRuntime: codex\nTool: {}\nDescription: {}\nInput: {}\nExpires: {}\nChoose an approval button for the scope shown above. Text replies are not approval.",
            target.authority.engagement_id,
            target.authority.project_id,
            request.method,
            description,
            preview,
            expires
        );
        let content = json!({"msgtype":"com.agentchat.approval.request.v1",
            "body":body,"com.agentchat.approval":detail});
        serde_json::to_writer(&mut EncodedLimit(MAX_CARD), &content)
            .map_err(|_| Error::Capacity)?;
        tx.commit()?;
        Ok(PrivateApprovalCard {
            target,
            owner_expires_at,
            content,
        })
    }

    pub fn check_private_approval_card(
        &mut self,
        card: &PrivateApprovalCard,
        now: u64,
    ) -> Result<(), Error> {
        self.check_private_approval_card_clock(card, || Ok(now))
    }

    pub(crate) fn check_private_approval_card_clock(
        &mut self,
        card: &PrivateApprovalCard,
        sample: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<(), Error> {
        let current = self.private_approval_card_clock(
            &card.target.request_id,
            card.owner_expires_at,
            sample,
        )?;
        if current.target != card.target || current.content != card.content {
            return Err(Error::Conflict);
        }
        Ok(())
    }
}
