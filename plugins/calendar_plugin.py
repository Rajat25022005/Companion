import json
import logging
from datetime import datetime, timedelta
from typing import Optional

from plugins.base_plugin import BasePlugin

logger = logging.getLogger(__name__)


class CalendarPlugin(BasePlugin):
    def __init__(self, config: Optional[dict] = None):
        super().__init__(name='calendar', config=config)
        self._service = None

    def _get_service(self):
        if self._service:
            return self._service
        try:
            from google.oauth2.credentials import Credentials
            from googleapiclient.discovery import build

            creds_path = self._config.get('credentials_path', '')
            creds = Credentials.from_authorized_user_file(creds_path, self._config.get('scopes', []))
            self._service = build('calendar', 'v3', credentials=creds)
            return self._service
        except Exception as e:
            logger.error('Calendar service init failed: %s', e)
            raise

    def get_tools(self) -> dict[str, callable]:
        return {
            'calendar_list_events': self.list_events,
            'calendar_create_event': self.create_event,
            'calendar_find_free_slots': self.find_free_slots,
        }

    def health_check(self) -> dict:
        try:
            service = self._get_service()
            cals = service.calendarList().list(maxResults=1).execute()
            return {'status': 'ok', 'calendars': len(cals.get('items', []))}
        except Exception as e:
            return {'status': 'error', 'error': str(e)}

    def list_events(self, start: str = '', end: str = '', max_results: int = 10) -> str:
        """List calendar events within a date range."""
        try:
            service = self._get_service()
            now = datetime.utcnow()
            time_min = start or now.isoformat() + 'Z'
            time_max = end or (now + timedelta(days=7)).isoformat() + 'Z'

            events = service.events().list(
                calendarId='primary', timeMin=time_min, timeMax=time_max,
                maxResults=max_results, singleEvents=True, orderBy='startTime',
            ).execute()

            items = []
            for event in events.get('items', []):
                items.append({
                    'id': event.get('id', ''),
                    'summary': event.get('summary', 'No title'),
                    'start': event.get('start', {}).get('dateTime', event.get('start', {}).get('date', '')),
                    'end': event.get('end', {}).get('dateTime', event.get('end', {}).get('date', '')),
                    'location': event.get('location', ''),
                })
            return json.dumps(items)
        except Exception as e:
            return json.dumps({'error': str(e)})

    def create_event(self, summary: str, start: str, end: str, description: str = '', location: str = '') -> str:
        """Create a calendar event (requires user confirmation)."""
        try:
            service = self._get_service()
            event = {
                'summary': summary,
                'start': {'dateTime': start, 'timeZone': 'Asia/Kolkata'},
                'end': {'dateTime': end, 'timeZone': 'Asia/Kolkata'},
            }
            if description:
                event['description'] = description
            if location:
                event['location'] = location

            created = service.events().insert(calendarId='primary', body=event).execute()
            return json.dumps({'status': 'created', 'id': created.get('id', ''), 'link': created.get('htmlLink', '')})
        except Exception as e:
            return json.dumps({'error': str(e)})

    def find_free_slots(self, date: str = '', duration_minutes: int = 60) -> str:
        """Find free time slots on a given date."""
        try:
            service = self._get_service()
            target = datetime.fromisoformat(date) if date else datetime.utcnow()
            day_start = target.replace(hour=9, minute=0, second=0)
            day_end = target.replace(hour=18, minute=0, second=0)

            events = service.events().list(
                calendarId='primary',
                timeMin=day_start.isoformat() + 'Z',
                timeMax=day_end.isoformat() + 'Z',
                singleEvents=True, orderBy='startTime',
            ).execute()

            busy = []
            for event in events.get('items', []):
                s = event.get('start', {}).get('dateTime', '')
                e = event.get('end', {}).get('dateTime', '')
                if s and e:
                    busy.append((datetime.fromisoformat(s.replace('Z', '')), datetime.fromisoformat(e.replace('Z', ''))))

            free = []
            current = day_start
            for bs, be in sorted(busy):
                if (bs - current).total_seconds() >= duration_minutes * 60:
                    free.append({'start': current.isoformat(), 'end': bs.isoformat()})
                current = max(current, be)
            if (day_end - current).total_seconds() >= duration_minutes * 60:
                free.append({'start': current.isoformat(), 'end': day_end.isoformat()})

            return json.dumps(free)
        except Exception as e:
            return json.dumps({'error': str(e)})
