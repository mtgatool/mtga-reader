/**
 * Simple demo showing how to use the mtga-reader package
 *
 * Run with: sudo node demo.js (macOS) or as Administrator (Windows)
 */

const mtga = require('mtga-reader');

async function main() {
    const isMacOS = process.platform === 'darwin';

    console.log('MTGA Reader Demo');
    console.log('-'.repeat(40));

    // Check prerequisites
    if (!mtga.isAdmin()) {
        console.error('Error: Administrator/root privileges required');
        process.exit(1);
    }

    if (!mtga.findProcess('MTGA')) {
        console.error('Error: MTGA is not running');
        process.exit(1);
    }

    // Initialize
    try {
        mtga.init('MTGA');
        console.log('Connected to MTGA');
    } catch (err) {
        console.error('Failed to connect:', err.message);
        process.exit(1);
    }

    // Read card collection
    console.log('\nReading card collection...');

    const path = isMacOS
        ? ['PAPA', '<InventoryManager>k__BackingField', '_inventoryServiceWrapper', '<Cards>k__BackingField']
        : ['WrapperController', '<Instance>k__BackingField', '<InventoryManager>k__BackingField', '_inventoryServiceWrapper', '<Cards>k__BackingField'];

    const cards = mtga.readData('MTGA', path);

    if (Array.isArray(cards)) {
        console.log(`Found ${cards.length} unique cards in collection!`);

        // Calculate total cards
        const total = cards.reduce((sum, c) => sum + c.value, 0);
        console.log(`Total cards: ${total}`);

        // Show sample
        console.log('\nSample cards:');
        cards.slice(0, 10).forEach(card => {
            console.log(`  Card ID ${card.key}: x${card.value}`);
        });
    } else if (cards && cards.error) {
        console.error('Error reading cards:', cards.error);
    }

    // Cleanup
    mtga.close();
    console.log('\nDone!');
}

main().catch(console.error);
