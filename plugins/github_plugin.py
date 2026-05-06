import json
import logging
import os
from typing import Optional

import httpx

from plugins.base_plugin import BasePlugin

logger = logging.getLogger(__name__)

GITHUB_API = 'https://api.github.com'


class GitHubPlugin(BasePlugin):
    def __init__(self, config: Optional[dict] = None):
        super().__init__(name='github', config=config)
        token_env = self._config.get('token_env', 'GITHUB_TOKEN')
        self._token = os.environ.get(token_env, '')

    def _headers(self) -> dict:
        h = {'Accept': 'application/vnd.github+json'}
        if self._token:
            h['Authorization'] = f'Bearer {self._token}'
        return h

    def get_tools(self) -> dict[str, callable]:
        return {
            'github_list_issues': self.list_issues,
            'github_read_pr': self.read_pr,
            'github_list_commits': self.list_commits,
            'github_get_file': self.get_file,
        }

    def health_check(self) -> dict:
        try:
            r = httpx.get(f'{GITHUB_API}/user', headers=self._headers(), timeout=10)
            if r.status_code == 200:
                return {'status': 'ok', 'user': r.json().get('login', '')}
            return {'status': 'error', 'code': r.status_code}
        except Exception as e:
            return {'status': 'error', 'error': str(e)}

    def list_issues(self, repo: str, state: str = 'open', max_results: int = 10) -> str:
        """List issues for a GitHub repository (owner/repo format)."""
        try:
            r = httpx.get(
                f'{GITHUB_API}/repos/{repo}/issues',
                headers=self._headers(), params={'state': state, 'per_page': max_results}, timeout=10,
            )
            issues = [
                {'number': i['number'], 'title': i['title'], 'state': i['state'],
                 'author': i.get('user', {}).get('login', ''), 'labels': [l['name'] for l in i.get('labels', [])]}
                for i in r.json()
            ]
            return json.dumps(issues)
        except Exception as e:
            return json.dumps({'error': str(e)})

    def read_pr(self, repo: str, pr_number: int) -> str:
        """Read a pull request's details and diff."""
        try:
            r = httpx.get(f'{GITHUB_API}/repos/{repo}/pulls/{pr_number}', headers=self._headers(), timeout=10)
            pr = r.json()
            return json.dumps({
                'number': pr.get('number'), 'title': pr.get('title'), 'state': pr.get('state'),
                'body': (pr.get('body') or '')[:2000], 'author': pr.get('user', {}).get('login', ''),
                'additions': pr.get('additions', 0), 'deletions': pr.get('deletions', 0),
                'changed_files': pr.get('changed_files', 0),
            })
        except Exception as e:
            return json.dumps({'error': str(e)})

    def list_commits(self, repo: str, max_results: int = 10) -> str:
        """List recent commits for a repository."""
        try:
            r = httpx.get(
                f'{GITHUB_API}/repos/{repo}/commits',
                headers=self._headers(), params={'per_page': max_results}, timeout=10,
            )
            commits = [
                {'sha': c['sha'][:7], 'message': c['commit']['message'].split('\n')[0],
                 'author': c['commit']['author']['name'], 'date': c['commit']['author']['date']}
                for c in r.json()
            ]
            return json.dumps(commits)
        except Exception as e:
            return json.dumps({'error': str(e)})

    def get_file(self, repo: str, path: str, ref: str = 'main') -> str:
        """Get file content from a GitHub repository."""
        try:
            r = httpx.get(
                f'{GITHUB_API}/repos/{repo}/contents/{path}',
                headers=self._headers(), params={'ref': ref}, timeout=10,
            )
            data = r.json()
            if 'content' in data:
                import base64
                content = base64.b64decode(data['content']).decode('utf-8', errors='replace')
                return json.dumps({'path': data.get('path', ''), 'content': content[:10000]})
            return json.dumps({'error': 'No content field in response'})
        except Exception as e:
            return json.dumps({'error': str(e)})
