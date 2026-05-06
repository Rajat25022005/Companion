import json
import logging
from typing import Optional

from plugins.base_plugin import BasePlugin

logger = logging.getLogger(__name__)


class GmailPlugin(BasePlugin):
    def __init__(self, config: Optional[dict] = None):
        super().__init__(name='gmail', config=config)
        self._service = None

    def _get_service(self):
        if self._service:
            return self._service

        try:
            from google.oauth2.credentials import Credentials
            from googleapiclient.discovery import build

            creds_path = self._config.get('credentials_path', '')
            if not creds_path:
                raise ValueError('No credentials_path configured for Gmail plugin.')

            creds = Credentials.from_authorized_user_file(creds_path, self._config.get('scopes', []))
            self._service = build('gmail', 'v1', credentials=creds)
            return self._service
        except Exception as e:
            logger.error('Gmail service init failed: %s', e)
            raise

    def get_tools(self) -> dict[str, callable]:
        return {
            'gmail_search': self.search_inbox,
            'gmail_read_thread': self.read_thread,
            'gmail_compose': self.compose,
            'gmail_reply': self.reply,
        }

    def health_check(self) -> dict:
        try:
            service = self._get_service()
            profile = service.users().getProfile(userId='me').execute()
            return {'status': 'ok', 'email': profile.get('emailAddress', '')}
        except Exception as e:
            return {'status': 'error', 'error': str(e)}

    def search_inbox(self, query: str, max_results: int = 10) -> str:
        """Search Gmail inbox by query string."""
        try:
            service = self._get_service()
            results = service.users().messages().list(
                userId='me', q=query, maxResults=max_results
            ).execute()
            messages = results.get('messages', [])

            summaries = []
            for msg in messages[:max_results]:
                detail = service.users().messages().get(
                    userId='me', id=msg['id'], format='metadata',
                    metadataHeaders=['Subject', 'From', 'Date']
                ).execute()
                headers = {h['name']: h['value'] for h in detail.get('payload', {}).get('headers', [])}
                summaries.append({
                    'id': msg['id'],
                    'subject': headers.get('Subject', ''),
                    'from': headers.get('From', ''),
                    'date': headers.get('Date', ''),
                    'snippet': detail.get('snippet', ''),
                })

            return json.dumps(summaries)
        except Exception as e:
            return json.dumps({'error': str(e)})

    def read_thread(self, thread_id: str) -> str:
        """Read a full email thread by thread ID."""
        try:
            service = self._get_service()
            thread = service.users().threads().get(userId='me', id=thread_id).execute()
            messages = []
            for msg in thread.get('messages', []):
                headers = {h['name']: h['value'] for h in msg.get('payload', {}).get('headers', [])}
                messages.append({
                    'from': headers.get('From', ''),
                    'date': headers.get('Date', ''),
                    'subject': headers.get('Subject', ''),
                    'snippet': msg.get('snippet', ''),
                })
            return json.dumps(messages)
        except Exception as e:
            return json.dumps({'error': str(e)})

    def compose(self, to: str, subject: str, body: str, cc: str = '', bcc: str = '') -> str:
        """Compose and save as draft (never auto-sends)."""
        try:
            import base64
            from email.mime.text import MIMEText

            message = MIMEText(body)
            message['to'] = to
            message['subject'] = subject
            if cc:
                message['cc'] = cc
            if bcc:
                message['bcc'] = bcc

            raw = base64.urlsafe_b64encode(message.as_bytes()).decode()
            service = self._get_service()
            draft = service.users().drafts().create(
                userId='me', body={'message': {'raw': raw}}
            ).execute()

            return json.dumps({'status': 'draft_created', 'draft_id': draft['id'], 'to': to, 'subject': subject})
        except Exception as e:
            return json.dumps({'error': str(e)})

    def reply(self, thread_id: str, body: str) -> str:
        """Reply to an existing thread (saved as draft)."""
        try:
            import base64
            from email.mime.text import MIMEText

            service = self._get_service()
            thread = service.users().threads().get(userId='me', id=thread_id).execute()
            last_msg = thread['messages'][-1]
            headers = {h['name']: h['value'] for h in last_msg.get('payload', {}).get('headers', [])}

            message = MIMEText(body)
            message['to'] = headers.get('From', '')
            message['subject'] = f'Re: {headers.get("Subject", "")}'
            message['In-Reply-To'] = headers.get('Message-ID', '')
            message['References'] = headers.get('Message-ID', '')

            raw = base64.urlsafe_b64encode(message.as_bytes()).decode()
            draft = service.users().drafts().create(
                userId='me', body={'message': {'raw': raw, 'threadId': thread_id}}
            ).execute()

            return json.dumps({'status': 'draft_created', 'draft_id': draft['id'], 'thread_id': thread_id})
        except Exception as e:
            return json.dumps({'error': str(e)})
