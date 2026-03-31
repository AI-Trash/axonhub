'use client';


import { formatNumber } from '@/utils/format-number';

import { useFastestChannels } from '../data/fastest-performers';
import type { FastestChannel } from '../data/fastest-performers';
import { FastestPerformersCard } from './fastest-performers-card';
import * as m from '@/paraglide/messages';

export function FastestChannelsCard() {
  return (
    <FastestPerformersCard<FastestChannel>
      title={m["dashboard.cards.fastestPerformers.channels"]()}
      description={(totalRequests) =>
        m["dashboard.cards.fastestPerformers.description"]({
          type: m["dashboard.cards.fastestPerformers.channelType"](),
          count: formatNumber(totalRequests) })
      }
      noDataLabel={m["dashboard.cards.fastestPerformers.noData"]()}
      useData={useFastestChannels}
      getName={(item) => item.channelName}
    />
  );
}
