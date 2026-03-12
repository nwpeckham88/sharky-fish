export function statusLabel(status: string | null | undefined): string {
	if (!status) return 'Unprocessed';
	return status
		.replaceAll('_', ' ')
		.toLowerCase()
		.replace(/\b\w/g, (value) => value.toUpperCase());
}

export function statusTone(status: string | null | undefined): string {
	switch (status) {
		case 'REVIEWED':
		case 'RE_SOURCE':
		case 'AWAITING_APPROVAL':
		case 'APPROVED':
		case 'PROCESSING':
			return 'processing';
		case 'PROCESSED':
		case 'COMPLETED':
		case 'KEPT_ORIGINAL':
			return 'completed';
		case 'FAILED':
		case 'REJECTED':
		case 'UNPROCESSED':
			return 'failed';
		default:
			return '';
	}
}

export function fileName(path: string): string {
	return path.split('/').pop() ?? path;
}

export function formatBytes(value: number): string {
	if (!value) return '0 B';
	const units = ['B', 'KB', 'MB', 'GB', 'TB'];
	let size = value;
	let unitIndex = 0;
	while (size >= 1024 && unitIndex < units.length - 1) {
		size /= 1024;
		unitIndex += 1;
	}
	return `${size.toFixed(size >= 10 || unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

export function formatTimestamp(value: number | null | undefined): string {
	if (!value) return 'Unknown';
	return new Date(value * 1000).toLocaleString();
}