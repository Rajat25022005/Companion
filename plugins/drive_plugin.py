import json
import logging
from typing import Optional

from plugins.base_plugin import BasePlugin

logger = logging.getLogger(__name__)


class DrivePlugin(BasePlugin):
    def __init__(self, config: Optional[dict] = None):
        super().__init__(name='drive', config=config)
        self._service = None

    def _get_service(self):
        if self._service:
            return self._service
        try:
            from google.oauth2.credentials import Credentials
            from googleapiclient.discovery import build
            creds_path = self._config.get('credentials_path', '')
            creds = Credentials.from_authorized_user_file(creds_path, self._config.get('scopes', []))
            self._service = build('drive', 'v3', credentials=creds)
            return self._service
        except Exception as e:
            logger.error('Drive service init failed: %s', e)
            raise

    def get_tools(self) -> dict[str, callable]:
        return {
            'drive_list_files': self.list_files,
            'drive_read_file': self.read_file,
            'drive_create_file': self.create_file,
            'drive_upload_file': self.upload_file,
        }

    def health_check(self) -> dict:
        try:
            service = self._get_service()
            about = service.about().get(fields='user').execute()
            return {'status': 'ok', 'user': about.get('user', {}).get('emailAddress', '')}
        except Exception as e:
            return {'status': 'error', 'error': str(e)}

    def list_files(self, query: str = '', max_results: int = 20) -> str:
        """List files in Google Drive, optionally filtered by query."""
        try:
            service = self._get_service()
            q = query or "trashed = false"
            results = service.files().list(
                q=q, pageSize=max_results,
                fields='files(id, name, mimeType, modifiedTime, size)',
            ).execute()
            return json.dumps(results.get('files', []))
        except Exception as e:
            return json.dumps({'error': str(e)})

    def read_file(self, file_id: str) -> str:
        """Read content of a Google Drive file by ID."""
        try:
            service = self._get_service()
            content = service.files().get_media(fileId=file_id).execute()
            return json.dumps({'file_id': file_id, 'content': content.decode('utf-8', errors='replace')[:10000]})
        except Exception as e:
            return json.dumps({'error': str(e)})

    def create_file(self, name: str, content: str, mime_type: str = 'text/plain') -> str:
        """Create a new file in Google Drive."""
        try:
            from googleapiclient.http import MediaInMemoryUpload
            service = self._get_service()
            media = MediaInMemoryUpload(content.encode('utf-8'), mimetype=mime_type)
            file_meta = {'name': name}
            created = service.files().create(body=file_meta, media_body=media, fields='id,name').execute()
            return json.dumps({'status': 'created', 'id': created['id'], 'name': created['name']})
        except Exception as e:
            return json.dumps({'error': str(e)})

    def upload_file(self, local_path: str, drive_name: str = '') -> str:
        """Upload a local file to Google Drive."""
        try:
            from pathlib import Path
            from googleapiclient.http import MediaFileUpload
            service = self._get_service()
            p = Path(local_path)
            if not p.exists():
                return json.dumps({'error': f'File not found: {local_path}'})
            name = drive_name or p.name
            media = MediaFileUpload(str(p))
            created = service.files().create(
                body={'name': name}, media_body=media, fields='id,name',
            ).execute()
            return json.dumps({'status': 'uploaded', 'id': created['id'], 'name': created['name']})
        except Exception as e:
            return json.dumps({'error': str(e)})
