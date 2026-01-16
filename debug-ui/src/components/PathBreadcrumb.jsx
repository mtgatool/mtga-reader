import React from 'react'
import './PathBreadcrumb.css'

function PathBreadcrumb({ path, onNavigate, pathError }) {
  if (!path || path.length === 0) return null

  const getSegmentIcon = (type) => {
    switch (type) {
      case 'assembly': return '📦'
      case 'class': return '📄'
      case 'static': return '🔹'
      case 'field': return '→'
      default: return '•'
    }
  }

  const getSegmentLabel = (segment) => {
    return segment.value
  }

  // Build the URL for copying
  const currentUrl = window.location.href

  const copyUrl = () => {
    navigator.clipboard.writeText(currentUrl)
      .then(() => {
        // Could add a toast notification here
        console.log('URL copied to clipboard')
      })
      .catch(err => {
        console.error('Failed to copy URL:', err)
      })
  }

  return (
    <div className="path-breadcrumb">
      <div className="breadcrumb-container">
        <span className="breadcrumb-label">Path:</span>
        <div className="breadcrumb-segments">
          {path.map((segment, index) => {
            const isLast = index === path.length - 1
            const isFailedSegment = pathError?.failedSegment === segment

            return (
              <React.Fragment key={index}>
                <span
                  className={`breadcrumb-segment ${isLast ? 'current' : ''} ${isFailedSegment ? 'failed' : ''}`}
                  onClick={() => !isLast && onNavigate(index)}
                  title={`${segment.type}: ${segment.value}`}
                >
                  <span className="segment-icon">{getSegmentIcon(segment.type)}</span>
                  <span className="segment-value">{getSegmentLabel(segment)}</span>
                </span>
                {!isLast && <span className="breadcrumb-separator">/</span>}
              </React.Fragment>
            )
          })}
        </div>
        <button className="copy-url-btn" onClick={copyUrl} title="Copy URL to clipboard">
          📋
        </button>
      </div>
      <div className="url-display">
        <code>{currentUrl}</code>
      </div>
    </div>
  )
}

export default PathBreadcrumb
