'use strict';

function retryDelay(attempt) {
  return Math.min(2 ** attempt * 100, 30000);
}

module.exports = { retryDelay };
