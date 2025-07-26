
function copyToClipboard(text, element) {
    navigator.clipboard.writeText(text).then(function() {
        const tooltip = element.parentElement.querySelector('.copy-tooltip');
        const originalBg = element.className;
        
        element.className = originalBg.replace('bg-gray-100', 'bg-green-100').replace('border-gray-200', 'border-green-300');
        tooltip.classList.remove('opacity-0');
        tooltip.classList.add('opacity-100');
        
        setTimeout(function() {
            element.className = originalBg;
            tooltip.classList.remove('opacity-100');
            tooltip.classList.add('opacity-0');
        }, 1500);
    }).catch(function() {
        console.error('Failed to copy text');
    });
}

async function showModelCoolings(keyId, keyName) {
    const modalKeyName = document.getElementById('modalKeyName');
    const modalTable = document.getElementById('modelCoolingsTable');
    const modal = document.getElementById('modelCoolingsModal');

    modalKeyName.textContent = keyName;
    modalTable.innerHTML = '<p class=\"text-gray-600 text-center py-8\">Loading...</p>';
    modal.classList.remove('hidden');
    modal.classList.add('flex');

    try {
        const response = await fetch(`/api/keys/${keyId}/coolings`);
        if (!response.ok) {
            throw new Error(`HTTP error! status: ${response.status}`);
        }
        const keyData = await response.json();
        const modelCoolings = keyData.model_coolings || {};
        const now = Date.now() / 1000;

        if (Object.keys(modelCoolings).length === 0) {
            modalTable.innerHTML = '<p class=\"text-gray-600 text-center py-8\">No model cooling data available</p>';
        } else {
            const rows = Object.entries(modelCoolings).map(([model, coolingEnd]) => {
                const isAvailable = coolingEnd < now;
                const remainingTime = isAvailable ? '-' : formatTime(coolingEnd - now);
                // We don't have total_seconds from this endpoint, so we can't display it.
                // You might need to adjust your API if this is required.
                const statusClass = isAvailable ? 'text-green-600 bg-green-50' : 'text-red-600 bg-red-50';
                const status = isAvailable ? 'available' : 'cooling';

                return `
                    <tr class=\"border-b border-gray-200\">
                        <td class=\"p-3 font-mono text-sm\">${model}</td>
                        <td class=\"p-3 text-sm\">${remainingTime}</td>
                        <td class=\"p-3\">
                            <span class=\"px-2 py-1 rounded-lg text-xs font-medium ${statusClass}\">${status}</span>
                        </td>
                    </tr>
                `;
            }).join('');

            modalTable.innerHTML = `
                <table class=\"w-full\">
                    <thead>
                        <tr class=\"border-b border-gray-200 bg-gray-50\">
                            <th class=\"p-3 text-left font-semibold text-gray-900\">Model</th>
                            <th class=\"p-3 text-left font-semibold text-gray-900\">Remaining Time</th>
                            <th class=\"p-3 text-left font-semibold text-gray-900\">Status</th>
                        </tr>
                    </thead>
                    <tbody>
                        ${rows}
                    </tbody>
                </table>
            `;
        }
    } catch (e) {
        console.error('Error fetching or parsing model cooling data:', e);
        modalTable.innerHTML = `<p class=\"text-red-600 text-center py-8\">Error: ${e.message}</p>`;
    }
}

function closeModal(event) {
    if (!event || event.target === event.currentTarget) {
        const modal = document.getElementById('modelCoolingsModal');
        modal.classList.add('hidden');
        modal.classList.remove('flex');
    }
}

function formatTime(seconds) {
    if (seconds <= 0) return '-';
    
    const days = Math.floor(seconds / 86400);
    const hours = Math.floor((seconds % 86400) / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    
    if (days > 0) {
        return `${days}d${hours}h`;
    }
    if (hours > 0) {
        return `${hours}h${minutes}m`;
    }
    return `${minutes}m`;
}
