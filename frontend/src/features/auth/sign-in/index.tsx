
import AuthLayout from '../auth-layout';
import TwoColumnAuth from '../components/two-column-auth';
import AnimatedLineBackground from './components/animated-line-background';
import { UserAuthForm } from './components/user-auth-form';

import './login-styles.css';
import * as m from '@/paraglide/messages';

export default function SignIn() {
  return (
    <AuthLayout>
      <div data-testid='sign-in-animation-layer'>
        <AnimatedLineBackground key='optimized-layout' />
      </div>
      <TwoColumnAuth
        title={m["auth.signIn.title"]()}
        description={m["auth.signIn.subtitle"]()}
        rightFooter={<p className='text-xs leading-relaxed text-slate-500 sm:text-sm'>{m["auth.signIn.footer.agreement"]()}</p>}
      >
        <UserAuthForm />
      </TwoColumnAuth>
    </AuthLayout>
  );
}
