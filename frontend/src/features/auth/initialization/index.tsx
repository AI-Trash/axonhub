
import AuthLayout from '../auth-layout';
import TwoColumnAuth from '../components/two-column-auth';
import AnimatedLineBackground from '../sign-in/components/animated-line-background';
import { InitializationForm } from './components/initialization-form';
import * as m from '@/paraglide/messages';

export default function Initialization() {
  return (
    <AuthLayout>
      <AnimatedLineBackground key='optimized-layout' />
      <TwoColumnAuth title={m["initialization.title"]()} description={m["initialization.description"]()} rightMaxWidthClassName='max-w-2xl'>
        <InitializationForm />
      </TwoColumnAuth>
    </AuthLayout>
  );
}
