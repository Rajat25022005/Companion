import { useState, useEffect } from 'react'

const API = '/api'

export function RepoSelector({ onClose, onIndexed }) {
  const [repos, setRepos] = useState([])
  const [loading, setLoading] = useState(true)
  const [search, setSearch] = useState('')
  const [indexing, setIndexing] = useState(null)
  const [error, setError] = useState(null)

  useEffect(() => {
    fetch(`${API}/github/repos`)
      .then(r => r.json())
      .then(d => { setRepos(d.repos || []); setLoading(false) })
      .catch(() => { setError('Failed to load repos. Set GITHUB_TOKEN in .env'); setLoading(false) })
  }, [])

  const filtered = repos.filter(r =>
    r.full_name?.toLowerCase().includes(search.toLowerCase())
  )

  const handleIndex = async (repo) => {
    setIndexing(repo.id)
    setError(null)
    try {
      const res = await fetch(`${API}/github/index-repo`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ clone_url: repo.clone_url, name: repo.name }),
      })
      if (!res.ok) {
        const d = await res.json()
        throw new Error(d.detail || 'Indexing failed')
      }
      const data = await res.json()
      onIndexed?.(repo.full_name, data.stats)
      onClose()
    } catch (e) {
      setError(e.message)
    } finally {
      setIndexing(null)
    }
  }

  return (
    <div style={styles.overlay} onClick={e => e.target === e.currentTarget && onClose()}>
      <div style={styles.modal}>
        <div style={styles.header}>
          <span style={styles.title}>SELECT REPO</span>
          <button style={styles.closeBtn} onClick={onClose}>×</button>
        </div>

        <input
          style={styles.search}
          placeholder="filter repos..."
          value={search}
          onChange={e => setSearch(e.target.value)}
          autoFocus
        />

        {error && <div style={styles.error}>{error}</div>}

        <div style={styles.list}>
          {loading && <div style={styles.empty}>loading repos...</div>}
          {!loading && filtered.length === 0 && (
            <div style={styles.empty}>no repos found</div>
          )}
          {filtered.map(repo => (
            <div key={repo.id} style={styles.repoRow}>
              <div style={styles.repoInfo}>
                <span style={styles.repoName}>{repo.full_name}</span>
                <span style={styles.repoMeta}>
                  {repo.language && `${repo.language} · `}
                  {repo.private ? 'private' : 'public'}
                  {repo.stargazers_count > 0 && ` · ★ ${repo.stargazers_count}`}
                </span>
                {repo.description && (
                  <span style={styles.repoDesc}>{repo.description}</span>
                )}
              </div>
              <button
                style={{
                  ...styles.indexBtn,
                  opacity: indexing === repo.id ? 0.5 : 1,
                }}
                onClick={() => handleIndex(repo)}
                disabled={indexing !== null}
              >
                {indexing === repo.id ? 'indexing...' : 'index'}
              </button>
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}

const styles = {
  overlay: {
    position: 'fixed', inset: 0,
    background: 'rgba(0,0,0,0.8)',
    display: 'flex', alignItems: 'center', justifyContent: 'center',
    zIndex: 1000,
  },
  modal: {
    background: '#0a0a0a',
    border: '1px solid #333',
    width: '560px', maxWidth: '90vw',
    maxHeight: '70vh',
    display: 'flex', flexDirection: 'column',
  },
  header: {
    display: 'flex', justifyContent: 'space-between', alignItems: 'center',
    padding: '14px 16px',
    borderBottom: '1px solid #333',
  },
  title: {
    fontSize: '11px', letterSpacing: '0.1em', color: '#888',
  },
  closeBtn: {
    background: 'transparent', border: 'none',
    color: '#fff', fontSize: '18px', cursor: 'pointer', lineHeight: 1,
  },
  search: {
    background: '#000', border: 'none', borderBottom: '1px solid #333',
    color: '#fff', padding: '12px 16px',
    fontFamily: 'inherit', fontSize: '13px', outline: 'none',
  },
  error: {
    padding: '10px 16px', fontSize: '12px',
    background: '#1a0000', color: '#ff6666',
    borderBottom: '1px solid #333',
  },
  list: {
    overflowY: 'auto', flex: 1,
  },
  empty: {
    padding: '24px', color: '#555',
    fontSize: '12px', textAlign: 'center',
  },
  repoRow: {
    display: 'flex', alignItems: 'center', gap: '12px',
    padding: '12px 16px', borderBottom: '1px solid #1a1a1a',
  },
  repoInfo: {
    flex: 1, display: 'flex', flexDirection: 'column', gap: '3px', minWidth: 0,
  },
  repoName: {
    fontSize: '13px', color: '#fff',
    whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
  },
  repoMeta: {
    fontSize: '10px', color: '#555', textTransform: 'uppercase',
  },
  repoDesc: {
    fontSize: '11px', color: '#666',
    whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
  },
  indexBtn: {
    background: 'transparent', border: '1px solid #333',
    color: '#fff', padding: '6px 14px',
    fontSize: '11px', textTransform: 'uppercase',
    letterSpacing: '0.05em', cursor: 'pointer',
    fontFamily: 'inherit', whiteSpace: 'nowrap',
    transition: 'all 0.15s',
  },
}
