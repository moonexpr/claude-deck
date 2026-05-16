"""API endpoints for Presence Dashboard — webhook receiver, REST, and WebSocket."""
import json

from fastapi import APIRouter, Depends, Request, WebSocket, WebSocketDisconnect
from sqlalchemy.ext.asyncio import AsyncSession

from app.config import settings
from app.database import get_db
from app.models.constants import SessionStatus
from app.models.schemas import (
    PresenceConfigSnippet,
    PresenceEventIn,
    PresenceSessionListResponse,
    PresenceSessionResponse,
    PresenceSessionUpdate,
)
from app.services.presence_service import PresenceService, manager

router = APIRouter()
service = PresenceService()


@router.post("/events")
async def receive_event(
    payload: PresenceEventIn,
    db: AsyncSession = Depends(get_db),
):
    """Webhook receiver for Claude Code HTTP hooks. Always returns {} with 200."""
    dumped = payload.model_dump()
    updated_session = await service.process_event(dumped, db)

    # Broadcast to WebSocket clients
    msg = json.dumps({"type": "session_update", "session": updated_session.model_dump()})
    await manager.broadcast(msg)

    return {}


@router.get("/sessions", response_model=PresenceSessionListResponse)
async def list_sessions(db: AsyncSession = Depends(get_db)):
    """Return all presence sessions."""
    sessions = await service.get_all_sessions(db)
    active = sum(1 for s in sessions if s.status == SessionStatus.ACTIVE)
    error = sum(1 for s in sessions if s.status == SessionStatus.ERROR)
    return PresenceSessionListResponse(
        sessions=sessions, total=len(sessions), active=active, error=error
    )


@router.patch("/sessions/{session_id}", response_model=PresenceSessionResponse)
async def update_session_label(
    session_id: str,
    update: PresenceSessionUpdate,
    db: AsyncSession = Depends(get_db),
):
    """Update a session's label."""
    result = await service.update_label(session_id, update.label, db)
    if not result:
        from fastapi import HTTPException
        raise HTTPException(status_code=404, detail="Session not found")
    return result


@router.delete("/sessions/{session_id}", status_code=204)
async def remove_session(
    session_id: str,
    db: AsyncSession = Depends(get_db),
):
    """Remove a session from the dashboard."""
    removed = await service.remove_session(session_id, db)
    if not removed:
        from fastapi import HTTPException
        raise HTTPException(status_code=404, detail="Session not found")

    # Broadcast removal
    msg = json.dumps({"type": "session_remove", "session_id": session_id})
    await manager.broadcast(msg)
    return None


@router.delete("/sessions", status_code=204)
async def clear_all_sessions(db: AsyncSession = Depends(get_db)):
    """Clear all sessions from the dashboard."""
    await service.clear_all_sessions(db)

    msg = json.dumps({"type": "sessions_cleared"})
    await manager.broadcast(msg)
    return None


@router.get("/config-snippet", response_model=PresenceConfigSnippet)
async def get_config_snippet(request: Request):
    """Generate the settings.json snippet for hooking up Claude Code.

    The events URL is resolved in priority order:
      1. settings.presence_public_url (env PRESENCE_PUBLIC_URL) — explicit override
      2. the request's external host/proto (Host + X-Forwarded-Proto) — so viewing
         the dialog through a reverse proxy yields that public hostname automatically
      3. http://localhost:8000 — local fallback
    """
    override = (settings.presence_public_url or "").strip().rstrip("/")
    if override:
        base = override
    else:
        host = request.headers.get("host", "localhost:8000")
        proto = request.headers.get("x-forwarded-proto", request.url.scheme)
        base = f"{proto}://{host}"
    url = f"{base}/api/v1/presence/events"
    events = [
        "Notification", "PreToolUse", "PostToolUse", "Stop",
        "SessionStart", "SessionEnd", "UserPromptSubmit",
        "SubagentStart", "SubagentStop",
    ]
    snippet = {
        "hooks": {
            event: [{"hooks": [{"type": "http", "url": url}]}]
            for event in events
        }
    }
    instructions = (
        "Add this to your ~/.claude/settings.json (or merge into existing hooks). "
        "Then restart any running Claude Code sessions for the hooks to take effect."
    )
    return PresenceConfigSnippet(snippet=snippet, instructions=instructions)


@router.websocket("/ws")
async def presence_websocket(
    ws: WebSocket,
    db: AsyncSession = Depends(get_db),
):
    """WebSocket for live presence updates. On connect, sends all current sessions."""
    await manager.connect(ws)
    try:
        # Send initial state
        sessions = await service.get_all_sessions(db)
        for s in sessions:
            await ws.send_text(
                json.dumps({"type": "session_update", "session": s.model_dump()})
            )

        # Keep connection alive — wait for disconnection
        while True:
            await ws.receive_text()
    except WebSocketDisconnect:
        pass
    finally:
        manager.disconnect(ws)
