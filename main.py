import logging
import subprocess
import sys
import threading
import time

logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s [%(name)s] %(levelname)s: %(message)s',
    datefmt='%H:%M:%S',
)
logger = logging.getLogger('companion')


def start_api():
    import uvicorn
    uvicorn.run(
        'api.server:app',
        host='0.0.0.0',
        port=8000,
        log_level='info',
    )


def start_ui():
    subprocess.run(
        ['npm', 'run', 'dev'],
        cwd='ui',
    )


def main():
    logger.info('Starting Companion...')
    logger.info('API server: http://localhost:8000')
    logger.info('UI: http://localhost:5173')

    api_thread = threading.Thread(target=start_api, daemon=True)
    api_thread.start()

    time.sleep(2)

    try:
        start_ui()
    except KeyboardInterrupt:
        logger.info('Shutting down.')


if __name__ == '__main__':
    main()
